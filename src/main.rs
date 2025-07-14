use actix_files::Files;
use actix_web::{get, web, App, HttpServer, HttpResponse, Result, Responder};
use actix_multipart::{Multipart, Field};
use futures_util::StreamExt;
use mysql::*;
use mysql::prelude::*;
use serde::Serialize;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};

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

    // 資産データを関数型で処理
    let asset_data: Result<Vec<Assets>, actix_web::Error> = assets_categories_rows
        .into_iter()
        .map(|row| {
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

            Ok(Assets {
                asset_name: asset_division_name,
                amount: round_two_digits(asset_amount),
                ratio: ratio(asset_amount, sum_amount),
                target_amount: round_two_digits(sum_amount * asset_target_ratio),
                target_ratio: format!("{}%", asset_target_ratio * 100.0),
            })
        })
        .collect();

    let rows: Vec<Assets> = asset_data?
        .into_iter()
        .chain(std::iter::once(Assets {
            asset_name: "合計金額".to_string(),
            amount: format!("{}", sum_amount),
            ratio: "100%".to_string(),
            target_amount: format!("{}", sum_amount),
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

    // 2. CSVをパース
    let file = File::open(&filepath)?;
    let reader = BufReader::new(file);
    let lines = reader.lines();

    println!("2. CSVをパース");

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
        if let Ok(row) = line {
            let cols: Vec<&str> = row.split(',').map(|s| s.trim()).collect();

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
    }

    tx.commit().map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    println!("4. 各行をINSERT");

    Ok(HttpResponse::Ok().body("ファイルアップロード完了"))

}