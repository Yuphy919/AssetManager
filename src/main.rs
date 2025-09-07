use actix_files::Files;
use actix_web::{get, web, App, HttpServer, HttpResponse, Result, Responder};
use actix_multipart::{Multipart, Field};
use futures_util::StreamExt;
use mysql::*;
use mysql::prelude::*;
use serde::Serialize;
use std::fs::File;
use std::io::{Write, Read};
use encoding_rs::*;

// =============================================================================
// Domain Layer - 純粋な関数型プログラミング
// =============================================================================

#[derive(Debug, Clone)]
struct AssetInfo {
    name: String,
    amount: f64,
    current_ratio: f64,
    target_ratio: f64,
}

#[derive(Debug)]
struct ProcessedAsset {
    name: String,
    amount: f64,
    ratio: f64,
    target_amount: f64,
    target_ratio: f64,
}

#[derive(Serialize)]
struct Assets {
    asset_name: String,
    amount: String,
    ratio: String,
    target_amount: String,
    target_ratio: String,
}

// ドメインロジック - 純粋関数
mod domain {
    use super::*;

    /**
     * 現在割合と目標割合の差が最も大きい資産を特定
     */
    pub fn find_max_diff_asset(assets: &[AssetInfo]) -> Option<&AssetInfo> {
        assets.iter()
            .max_by(|a, b| {
                let diff_a = (a.current_ratio - a.target_ratio).abs();
                let diff_b = (b.current_ratio - b.target_ratio).abs();
                diff_a.partial_cmp(&diff_b).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /**
     * 基準資産から目標総資産額を計算
     */
    pub fn calculate_base_total_amount(base_asset: &AssetInfo, current_total: f64) -> f64 {
        if base_asset.target_ratio > 0.0 {
            base_asset.amount / base_asset.target_ratio
        } else {
            current_total
        }
    }

    /**
     * 資産情報を処理済み資産データに変換
     */
    pub fn process_assets(assets: Vec<AssetInfo>, base_total_amount: f64) -> Vec<ProcessedAsset> {
        assets.into_iter()
            .map(|asset| ProcessedAsset {
                name: asset.name,
                amount: asset.amount,
                ratio: asset.current_ratio,
                target_amount: base_total_amount * asset.target_ratio,
                target_ratio: asset.target_ratio,
            })
            .collect()
    }

    /**
     * 処理済み資産データをレスポンス形式に変換
     */
    pub fn format_response(processed_assets: Vec<ProcessedAsset>, total_amount: f64, base_total_amount: f64) -> Vec<Assets> {
        let mut result: Vec<Assets> = processed_assets.into_iter()
            .map(|asset| Assets {
                asset_name: asset.name,
                amount: round_two_digits(asset.amount),
                ratio: format!("{}%", round_two_digits(asset.ratio * 100.0)),
                target_amount: round_two_digits(asset.target_amount),
                target_ratio: format!("{}%", round_two_digits(asset.target_ratio * 100.0)),
            })
            .collect();

        // 合計行を追加
        result.push(Assets {
            asset_name: "合計金額".to_string(),
            amount: round_two_digits(total_amount),
            ratio: "100%".to_string(),
            target_amount: round_two_digits(base_total_amount),
            target_ratio: "100%".to_string(),
        });

        result
    }
}

// =============================================================================
// Repository Layer - 命令型プログラミング（データアクセス）
// =============================================================================

mod repository {
    use super::*;

    /**
     * データベース接続を取得
     */
    pub fn get_connection() -> Result<PooledConn, Box<dyn std::error::Error>> {
        let url = "mysql://root:root_pass@localhost:3306/my_assets";
        let pool = Pool::new(url)?;
        let conn = pool.get_conn()?;
        Ok(conn)
    }

    /**
     * 総資産額を取得
     */
    pub fn get_total_assets(conn: &mut PooledConn) -> Result<f64, Box<dyn std::error::Error>> {
        let rows: Vec<Row> = conn.query("SELECT SUM(amount) FROM assets")?;
        let total = rows.into_iter()
            .next()
            .and_then(|row| row.get(0))
            .unwrap_or_default();
        Ok(total)
    }

    /**
     * 資産区分データを取得
     */
    pub fn get_asset_categories(conn: &mut PooledConn) -> Result<Vec<Row>, Box<dyn std::error::Error>> {
        let rows = conn.query(
            "SELECT asset_categories.division, asset_categories.name, target_percentage.ratio As target_ratio 
             FROM asset_categories 
             LEFT JOIN target_percentage 
             ON asset_categories.division = target_percentage.asset_division"
        )?;
        Ok(rows)
    }

    /**
     * 資産区分別の合計金額を取得
     */
    pub fn get_asset_amount_by_division(conn: &mut PooledConn, division: i32) -> Result<f64, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT SUM(assets.amount) AS amount 
             FROM assets 
             INNER JOIN asset_master 
             ON assets.id = asset_master.id 
             AND asset_master.division = {}", 
            division
        );
        let rows: Vec<Row> = conn.query(query)?;
        let amount = rows.into_iter()
            .next()
            .and_then(|row| row.get("amount"))
            .unwrap_or_default();
        Ok(amount)
    }

    /**
     * 資産データを挿入
     */
    pub fn insert_asset(tx: &mut Transaction, id: i32, amount: f64) -> Result<(), Box<dyn std::error::Error>> {
        tx.exec_drop("INSERT INTO assets (id, amount) VALUES (?, ?)", (id, amount))?;
        Ok(())
    }

    /**
     * 資産マスターからIDを取得
     */
    pub fn get_asset_id_by_name(tx: &mut Transaction, name: &str) -> Result<Option<i32>, Box<dyn std::error::Error>> {
        let result = tx.exec_first("SELECT id FROM asset_master WHERE name = ?", (name,))?;
        Ok(result)
    }

    /**
     * 既存資産データを削除
     */
    pub fn delete_all_assets(tx: &mut Transaction) -> Result<(), Box<dyn std::error::Error>> {
        tx.exec_drop("DELETE FROM assets", ())?;
        Ok(())
    }
}

// =============================================================================
// Service Layer - ビジネスロジック（マルチパラダイム）
// =============================================================================

mod service {
    use super::*;

    /**
     * 全資産データを取得・処理
     */
    pub async fn get_all_assets() -> Result<Vec<Assets>, actix_web::Error> {
        // データ取得（命令型）
        let mut conn = repository::get_connection()
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        let total_amount = repository::get_total_assets(&mut conn)
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        let category_rows = repository::get_asset_categories(&mut conn)
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        // データ変換（関数型）
        let asset_info: Result<Vec<AssetInfo>, actix_web::Error> = category_rows.iter()
            .map(|row| {
                let division: i32 = row.get("division").unwrap_or_default();
                let name: String = row.get("name").unwrap_or_default();
                let target_ratio: f64 = row.get("target_ratio").unwrap_or_default();

                let amount = repository::get_asset_amount_by_division(&mut conn, division)
                    .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

                let current_ratio = if total_amount > 0.0 { amount / total_amount } else { 0.0 };

                Ok(AssetInfo {
                    name,
                    amount,
                    current_ratio,
                    target_ratio,
                })
            })
            .collect();

        let asset_info = asset_info?;

        // ビジネスロジック（関数型）
        let base_total_amount = domain::find_max_diff_asset(&asset_info)
            .map(|asset| domain::calculate_base_total_amount(asset, total_amount))
            .unwrap_or(total_amount);

        let processed_assets = domain::process_assets(asset_info, base_total_amount);
        let response = domain::format_response(processed_assets, total_amount, base_total_amount);

        Ok(response)
    }

    /**
     * CSVファイルをアップロード・処理
     */
    pub async fn upload_csv(payload: Multipart) -> Result<String, actix_web::Error> {
        // ファイル保存（命令型）
        let filepath = save_uploaded_file(payload).await?;

        // ファイル処理（関数型）
        let lines = detect_encoding_and_read_lines(&filepath)
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        let parsed_data = parse_csv_lines(lines)?;

        // データベース保存（命令型）
        save_assets_to_database(parsed_data).await?;

        Ok("ファイルアップロード完了".to_string())
    }

    // プライベート関数群
    async fn save_uploaded_file(mut payload: Multipart) -> Result<String, actix_web::Error> {
        let mut filepath = String::new();
        while let Some(item) = payload.next().await {
            let mut field: Field = item.map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
            let content_disposition = field.content_disposition();
            let filename = content_disposition.get_filename().unwrap();
            filepath = format!("./uploads/{}", sanitize_filename::sanitize(filename));

            let mut f = File::create(&filepath)
                .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
            while let Some(chunk) = field.next().await {
                let data = chunk.map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
                f.write_all(&data)
                    .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
            }
        }
        Ok(filepath)
    }

    fn parse_csv_lines(lines: Vec<String>) -> Result<Vec<(String, f64)>, actix_web::Error> {
        let parsed: Vec<(String, f64)> = lines.into_iter()
            .filter_map(|line| {
                let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                if cols.len() == 11 {
                    let name = cols[0].trim_matches('"').to_string();
                    let profit: f64 = cols[9].parse().unwrap_or(0.0);
                    if profit != 0.0 {
                        Some((name, profit))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        Ok(parsed)
    }

    async fn save_assets_to_database(assets: Vec<(String, f64)>) -> Result<(), actix_web::Error> {
        let mut conn = repository::get_connection()
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        let mut tx = conn.start_transaction(TxOpts::default())
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        repository::delete_all_assets(&mut tx)
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        for (name, amount) in assets {
            let id = repository::get_asset_id_by_name(&mut tx, &name)
                .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?
                .ok_or_else(|| actix_web::error::ErrorBadRequest("資産が見つかりません"))?;

            repository::insert_asset(&mut tx, id, amount)
                .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

        Ok(())
    }
}

// =============================================================================
// Presentation Layer - Web API（オブジェクト指向的）
// =============================================================================

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .route("/upload", web::post().to(upload))
            .service(view_assets_api)
            .service(Files::new("/", "./static").index_file("index.html"))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

#[get("/api/view_assets")]
async fn view_assets_api() -> Result<impl Responder, actix_web::Error> {
    let assets = service::get_all_assets().await?;
    Ok(HttpResponse::Ok().json(assets))
}

async fn upload(payload: Multipart) -> Result<HttpResponse, actix_web::Error> {
    let message = service::upload_csv(payload).await?;
    Ok(HttpResponse::Ok().body(message))
}

// =============================================================================
// Utility Functions - 純粋関数
// =============================================================================

/**
 * 数値を小数点以下2桁で四捨五入して文字列に変換する関数
 */
fn round_two_digits(target: f64) -> String {
    format!("{}", (target * 100.0).round() / 100.0)
}

/**
 * エンコーディングを自動検出してCSVファイルを読み込む関数
 * UTF-8、UTF-16、Shift_JIS（ANSI）に対応
 */
fn detect_encoding_and_read_lines(filepath: &str) -> std::io::Result<Vec<String>> {
    let mut file = File::open(filepath)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    
    // BOM（Byte Order Mark）の検出でエンコーディングを判定
    let (encoding, _) = if buffer.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (UTF_8, &buffer[3..])   // UTF-8 BOM
    } else if buffer.starts_with(&[0xFF, 0xFE]) {
        (UTF_16LE, &buffer[2..]) // UTF-16 Little Endian BOM
    } else if buffer.starts_with(&[0xFE, 0xFF]) {
        (UTF_16BE, &buffer[2..]) // UTF-16 Big Endian BOM
    } else {
        // BOMがない場合の推測処理
        match std::str::from_utf8(&buffer) {
            Ok(_) => (UTF_8, &buffer[..]),          // UTF-8として有効
            Err(_) => (SHIFT_JIS, &buffer[..]),     // Shift_JIS（ANSI）として処理
        }
    };
    
    // 検出したエンコーディングで文字列にデコード
    let (decoded_text, _, _) = encoding.decode(&buffer);
    
    // 行単位で分割してベクターに格納
    let lines: Vec<String> = decoded_text
        .lines()
        .map(|line| line.to_string())
        .collect();
    
    Ok(lines)
}