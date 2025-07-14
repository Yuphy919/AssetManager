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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            // staticフォルダの中身を `/` で配信
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
    #[derive(Serialize)]
    struct Assets {
        asset_name: String,
        amount: String,
        ratio: String,
        target_amount:String,
        target_ratio:String,
    }

    let url = "mysql://root:root_pass@localhost:3306/my_assets"; // 環境に合わせて修正
    let pool = Pool::new(url).map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
    let mut conn = pool.get_conn().map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    // クエリ実行してVecに格納
    let sum_amount_rows: Vec<Row> = conn.query("SELECT SUM(amount) FROM assets")
        .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
    
    let sum_amount: f64 = sum_amount_rows.into_iter()
        .next()
        .and_then(|row| row.get(0))
        .unwrap_or_default();

    // 資産区分別
    let assets_categories_rows: Vec<Row> = conn.query("SELECT asset_categories.division, asset_categories.name, target_percentage.ratio As target_ratio FROM asset_categories LEFT JOIN target_percentage ON asset_categories.division = target_percentage.asset_division")
        .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    // 最初に全ての資産データを収集し、割合の差を計算
    let mut asset_info: Vec<(String, f64, f64, f64)> = Vec::new(); // (name, amount, current_ratio, target_ratio)
    
    for row in &assets_categories_rows {
        let asset_division: i32 = row.get("division").unwrap_or_default();
        let asset_division_name: String = row.get("name").unwrap_or_default();
        let query = format!("SELECT SUM(assets.amount) AS amount FROM assets INNER JOIN asset_master ON assets.id = asset_master.id AND asset_master.division = {}", asset_division);
        let asset_amount_rows: Vec<Row> = conn.query(query)
            .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
        let asset_amount: f64 = asset_amount_rows.into_iter()
            .next()
            .and_then(|row| row.get("amount"))
            .unwrap_or_default();
        let asset_target_ratio: f64 = row.get("target_ratio").unwrap_or_default();
        let current_ratio = if sum_amount > 0.0 { asset_amount / sum_amount } else { 0.0 };
        
        asset_info.push((asset_division_name, asset_amount, current_ratio, asset_target_ratio));
    }

    // 割合の差が最も大きい資産を見つける
    let max_diff_asset = asset_info.iter()
        .max_by(|a, b| {
            let diff_a = (a.2 - a.3).abs(); // |current_ratio - target_ratio|
            let diff_b = (b.2 - b.3).abs();
            diff_a.partial_cmp(&diff_b).unwrap_or(std::cmp::Ordering::Equal)
        });

    // 基準となる総資産額を計算
    let base_total_amount = if let Some(base_asset) = max_diff_asset {
        if base_asset.3 > 0.0 { // target_ratio > 0
            base_asset.1 / base_asset.3 // amount / target_ratio
        } else {
            sum_amount
        }
    } else {
        sum_amount
    };

    // 資産データを関数型で処理
    let asset_data: Result<Vec<Assets>, actix_web::Error> = asset_info.into_iter()
        .map(|(asset_division_name, asset_amount, _current_ratio, asset_target_ratio)| {
            Ok(Assets {
                asset_name: asset_division_name,
                amount: round_two_digits(asset_amount),
                ratio: ratio(asset_amount, sum_amount),
                target_amount: round_two_digits(base_total_amount * asset_target_ratio),
                target_ratio: format!("{}%", asset_target_ratio * 100.0),
            })
        })
        .collect();

    let rows: Vec<Assets> = asset_data?
        .into_iter()
        .chain(std::iter::once(Assets {
            asset_name: "合計金額".to_string(),
            amount: round_two_digits(sum_amount),
            ratio: "100%".to_string(),
            target_amount: round_two_digits(base_total_amount),
            target_ratio: "100%".to_string(),
        }))
        .collect();
    
    Ok(HttpResponse::Ok().json(rows))

}

fn ratio(asset_amount:f64,sum_amount:f64) -> String {
    let ratio:f64 = asset_amount / sum_amount * 100.0;
    format!("{}%",round_two_digits(ratio)).to_string()
}

fn round_two_digits(target:f64) ->String {
    format!("{}",(target* 100.0).round() / 100.0).to_string()
}

// エンコーディング検出と変換の関数
fn detect_encoding_and_read_lines(filepath: &str) -> std::io::Result<Vec<String>> {
    let mut file = File::open(filepath)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    
    // BOMの検出
    let (encoding, _) = if buffer.starts_with(&[0xEF, 0xBB, 0xBF]) {
        // UTF-8 BOM
        (UTF_8, &buffer[3..])
    } else if buffer.starts_with(&[0xFF, 0xFE]) {
        // UTF-16 LE BOM
        (UTF_16LE, &buffer[2..])
    } else if buffer.starts_with(&[0xFE, 0xFF]) {
        // UTF-16 BE BOM
        (UTF_16BE, &buffer[2..])
    } else {
        // BOMなし、エンコーディングを推測
        // 日本語の場合、Shift_JIS (ANSI) または UTF-8 の可能性が高い
        // 最初にUTF-8として試し、失敗したらShift_JISとして試す
        match std::str::from_utf8(&buffer) {
            Ok(_) => (UTF_8, &buffer[..]),
            Err(_) => (SHIFT_JIS, &buffer[..]), // ANSIの場合、通常はShift_JIS
        }
    };
    
    // デコードして文字列に変換
    let (decoded_text, _, _) = encoding.decode(&buffer);
    
    // 行に分割
    let lines: Vec<String> = decoded_text
        .lines()
        .map(|line| line.to_string())
        .collect();
    
    Ok(lines)
}

async fn upload(mut payload: Multipart) -> Result<HttpResponse> {

    // 1. アップロードされたCSVを保存
    let mut filepath = String::new();

    while let Some(item) = payload.next().await {
        let mut field: Field = item?;

        let content_disposition = field.content_disposition();
        let filename = content_disposition.get_filename().unwrap();
        filepath = format!("./uploads/{}", sanitize_filename::sanitize(filename));

        let mut f = File::create(&filepath)?;

        while let Some(chunk) = field.next().await {
            let data = chunk?;
            f.write_all(&data)?; 
        }
    }

    println!("1. アップロードされたCSVを保存");

    // 2. CSVをパース（エンコーディング自動検出）
    let lines = detect_encoding_and_read_lines(&filepath)
        .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    println!("2. CSVをパース（エンコーディング: 自動検出）");

    // 3. MySQLへ接続
    let url = "mysql://root:root_pass@localhost:3306/my_assets"; // 環境に合わせて修正
    let pool = Pool::new(url).map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
    let mut conn = pool.get_conn().map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    // トランザクション開始
    let mut tx = conn.start_transaction(TxOpts::default()).map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    println!("3. MySQLへ接続");

    tx.exec_drop("DELETE FROM assets",()).unwrap();

    // 4. 各行をINSERT
    for line in lines {
        let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();

        if cols.len() != 11 {
            continue; // 必要な列数に満たない場合はスキップ
        }
        
        let name = cols[0].trim_matches('"');
        let profit: f64 = cols[9].parse().unwrap_or(0.0); // 損益列

        if profit.to_string() == "0" {
            continue; // 必要な列数に満たない場合はスキップ
        }

        let id : i32 = tx.exec_first("SELECT id FROM asset_master WHERE name = ?", (name,))
                        .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))? // Result<Option<i32>>
                        .ok_or_else(|| actix_web::error::ErrorBadRequest("資産が見つかりません"))?; // Option → Result
        println!("{}{}{}", id, ":", name);

        tx.exec_drop(
            "INSERT INTO assets (id,  amount) VALUES (?, ?)",
            (id, profit),
        ).unwrap();
    }

    tx.commit().map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    println!("4. 各行をINSERT");

    Ok(HttpResponse::Ok().body("ファイルアップロード完了"))

}