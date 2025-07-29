# AssetManager - 資産配分管理システム 仕様書

## 📋 システム概要

### システム名
**AssetManager** - 資産配分管理・分析システム

### 目的
CSVファイルから資産データを読み込み、設定された目標割合に基づいて資産配分の分析・可視化を行うWebアプリケーション

### 対象ユーザー
- 自分

---

## 🎯 機能要件

### 1. CSVファイルアップロード機能
- **機能ID**: F001
- **概要**: 資産データをCSVファイル形式で一括登録
- **詳細**:
  - ドラッグ&ドロップによるファイルアップロード
  - 複数エンコーディング対応（UTF-8、UTF-16、Shift_JIS）
  - 11列固定形式のCSVファイル
  - 資産名と金額データの抽出・検証

### 2. 資産配分表示機能
- **機能ID**: F002
- **概要**: 現在の資産配分と目標配分を表形式で表示
- **詳細**:
  - 資産区分別の保有金額表示
  - 現在割合と目標割合の比較
  - 目標金額の自動計算
  - 合計金額行の表示

### 3. 目標金額計算機能
- **機能ID**: F003
- **概要**: 最適な目標金額を自動算出
- **詳細**:
  - 現在割合と目標割合の差が最大の資産を基準として設定
  - 基準資産から理想的な総資産額を逆算
  - 各資産区分の目標金額を算出

### 4. グラフ表示機能
- **機能ID**: F004
- **概要**: 資産配分を円グラフで可視化
- **詳細**:
  - Chart.jsによる動的グラフ生成
  - CSVアップロード後の自動更新
  - レスポンシブ対応

---

## 🏗️ 技術仕様

### アーキテクチャ
```
┌─────────────────────────────────────────┐
│  Presentation Layer (Web API)           │
├─────────────────────────────────────────┤
│  Service Layer (Business Logic)        │
├─────────────────────────────────────────┤
│  Repository Layer (Data Access)        │
├─────────────────────────────────────────┤
│  Domain Layer (Pure Functions)         │
└─────────────────────────────────────────┘
```

### 技術スタック
- **バックエンド**: Rust (Actix-Web)
- **データベース**: MySQL 8.0+
- **フロントエンド**: HTML5, CSS3, JavaScript (ES6+)
- **グラフライブラリ**: Chart.js
- **CSS設計**: OOCSS (Object Oriented CSS)

### 依存関係
```toml
[dependencies]
actix-web = "4.0"
actix-files = "0.6"
actix-multipart = "0.6"
mysql = "24.0"
serde = { version = "1.0", features = ["derive"] }
futures-util = "0.3"
encoding_rs = "0.8"
sanitize-filename = "0.4"
```

---

## 🗄️ データベース設計

### テーブル構成

#### 1. assets（資産データ）
| カラム名 | データ型 | 制約 | 説明 |
|----------|----------|------|------|
| id | INT | FK | 資産ID |
| amount | DECIMAL(15,2) | NOT NULL | 保有金額 |

#### 2. asset_master（資産マスター）
| カラム名 | データ型 | 制約 | 説明 |
|----------|----------|------|------|
| id | INT | PK | 資産ID |
| name | VARCHAR(100) | NOT NULL | 資産名 |
| division | INT | FK | 資産区分ID |

#### 3. asset_categories（資産区分マスター）
| カラム名 | データ型 | 制約 | 説明 |
|----------|----------|------|------|
| division | INT | PK | 資産区分ID |
| name | VARCHAR(50) | NOT NULL | 資産区分名 |

#### 4. target_percentage（目標割合設定）
| カラム名 | データ型 | 制約 | 説明 |
|----------|----------|------|------|
| asset_division | INT | FK | 資産区分ID |
| ratio | DECIMAL(5,4) | NOT NULL | 目標割合（小数） |

### データベース作成スクリプト
```sql
-- データベース作成
CREATE DATABASE my_assets;
USE my_assets;

-- 資産区分マスターテーブル
CREATE TABLE asset_categories (
    division INT PRIMARY KEY,
    name VARCHAR(50) NOT NULL
);

-- 資産マスターテーブル
CREATE TABLE asset_master (
    id INT PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    division INT,
    FOREIGN KEY (division) REFERENCES asset_categories(division)
);

-- 資産データテーブル
CREATE TABLE assets (
    id INT,
    amount DECIMAL(15,2) NOT NULL,
    FOREIGN KEY (id) REFERENCES asset_master(id)
);

-- 目標割合設定テーブル
CREATE TABLE target_percentage (
    asset_division INT,
    ratio DECIMAL(5,4) NOT NULL,
    FOREIGN KEY (asset_division) REFERENCES asset_categories(division)
);

-- サンプルデータ挿入
INSERT INTO asset_categories (division, name) VALUES
(1, '日本株式'),
(2, '米国株式'),
(3, '債券'),
(4, 'REIT');

INSERT INTO asset_master (id, name, division) VALUES
(1, '日経225連動型ETF', 1),
(2, 'S&P500連動型ETF', 2),
(3, '国内債券ETF', 3),
(4, 'J-REIT ETF', 4);

INSERT INTO target_percentage (asset_division, ratio) VALUES
(1, 0.30),  -- 日本株式 30%
(2, 0.40),  -- 米国株式 40%
(3, 0.20),  -- 債券 20%
(4, 0.10);  -- REIT 10%
```

---

## 🔌 API仕様

### 1. 資産データ取得API
- **エンドポイント**: `GET /api/view_assets`
- **概要**: 全資産データを取得・計算して返却
- **レスポンス形式**: JSON
```json
[
  {
    "asset_name": "日本株式",
    "amount": "1000000",
    "ratio": "60.0%",
    "target_amount": "500000",
    "target_ratio": "30.0%"
  },
  {
    "asset_name": "合計金額",
    "amount": "1666666",
    "ratio": "100%",
    "target_amount": "1666666",
    "target_ratio": "100%"
  }
]
```

### 2. CSVアップロードAPI
- **エンドポイント**: `POST /upload`
- **概要**: CSVファイルをアップロードして資産データを更新
- **リクエスト形式**: multipart/form-data
- **レスポンス**: プレーンテキスト
```
ファイルアップロード完了
```

### 3. 静的ファイル配信
- **エンドポイント**: `GET /`
- **概要**: HTML、CSS、JavaScriptファイルを配信
- **ルートファイル**: `index.html`

---

## 📊 ビジネスロジック仕様

### 目標金額計算アルゴリズム

#### Step 1: 最大乖離資産の特定
```rust
// 各資産の現在割合と目標割合の差の絶対値を計算
diff = |current_ratio - target_ratio|

// 差が最大の資産を特定
max_diff_asset = assets.max_by(|a, b| diff_a.cmp(diff_b))
```

#### Step 2: 基準総資産額の算出
```rust
// 基準資産の金額を目標割合で割って理想的な総資産額を計算
base_total_amount = max_diff_asset.amount / max_diff_asset.target_ratio
```

#### Step 3: 各資産の目標金額計算
```rust
// 基準総資産額に各資産の目標割合を掛けて目標金額を算出
target_amount = base_total_amount × target_ratio
```

### 計算例
| 資産区分 | 現在金額 | 現在割合 | 目標割合 | 差の絶対値 |
|----------|----------|----------|----------|------------|
| 日本株式 | 500万円 | 50% | 30% | **20%** |
| 米国株式 | 300万円 | 30% | 40% | 10% |
| 債券 | 200万円 | 20% | 30% | 10% |

1. **最大乖離**: 日本株式（20%の差）
2. **基準総資産額**: 500万円 ÷ 0.3 = 1,667万円
3. **目標金額**:
   - 日本株式: 1,667万円 × 0.3 = 500万円
   - 米国株式: 1,667万円 × 0.4 = 667万円
   - 債券: 1,667万円 × 0.3 = 500万円

---

## 📁 ファイル仕様

### CSVファイル形式
- **ファイル形式**: CSV（カンマ区切り）
- **エンコーディング**: UTF-8、UTF-16、Shift_JIS対応
- **列数**: 11列固定
- **必要データ**:
  - 1列目: 資産名（ダブルクォート囲み）
  - 10列目: 金額（数値）

### CSVサンプル
```csv
"日本株式",,,,,,,,,1000000,
"米国株式",,,,,,,,,500000,
"債券",,,,,,,,,300000,
"REIT",,,,,,,,,200000,
```

### アップロードディレクトリ
- **パス**: `uploads/`
- **ファイル名**: サニタイズ後のオリジナルファイル名
- **権限**: 読み書き可能

---

## 🎨 UI/UX仕様

### デザインシステム
- **CSS設計**: OOCSS（Object Oriented CSS）
- **レスポンシブ**: モバイルファーストデザイン
- **カラーパレット**: 
  - プライマリ: `#2c3e50`
  - セカンダリ: `#3498db`
  - アクセント: `#e74c3c`

### レイアウト構成
```
┌─────────────────────────────────────┐
│             Header                  │
├─────────────────────────────────────┤
│  Upload Area    │                   │
│  (Drag & Drop)  │                   │
├─────────────────┤     Chart         │
│                 │   (Pie Chart)     │
│    Data Table   │                   │
│                 │                   │
└─────────────────┴───────────────────┘
```

### インタラクション
- **ファイルドロップ**: ドラッグ中のビジュアルフィードバック
- **アップロード完了**: 自動テーブル・グラフ更新
- **レスポンシブ**: タブレット・スマートフォン対応

---

## 🔒 セキュリティ仕様

### ファイルアップロード
- **ファイルサイズ制限**: デフォルト（Actix-Webの設定に依存）
- **ファイル名サニタイズ**: `sanitize-filename`ライブラリ使用
- **許可拡張子**: `.csv`のみ（コンテンツタイプチェック推奨）

### データベース
- **SQLインジェクション対策**: プリペアドステートメント使用
- **接続情報**: 開発環境用（本番環境では環境変数推奨）
  - ホスト: `localhost:3306`
  - ユーザー: `root`
  - パスワード: `root_pass`
  - データベース: `my_assets`

### エラーハンドリング
- **内部エラー**: ログ出力のみ、詳細情報は非表示
- **ユーザーエラー**: 適切なエラーメッセージ表示

---

## ⚡ パフォーマンス仕様

### レスポンス時間
- **API応答時間**: 500ms以内（通常負荷時）
- **ファイルアップロード**: 10MB以下のファイルで5秒以内
- **グラフ描画**: 1秒以内

### メモリ使用量
- **アイドル時**: 50MB以下
- **ファイル処理時**: 100MB以下

### 同時接続
- **想定同時ユーザー数**: 10ユーザー
- **データベース接続**: コネクションプール使用

---

## 🧪 テスト仕様

### テストケース分類
1. **単体テスト**: Domain Layer関数のテスト
2. **統合テスト**: API エンドポイントのテスト
3. **E2Eテスト**: ブラウザ操作のテスト

### テストデータ
```csv
"テスト株式A",,,,,,,,,1000000,
"テスト株式B",,,,,,,,,500000,
"テスト債券",,,,,,,,,300000,
```

---

## 🚀 運用仕様

### システム要件
- **OS**: Windows 10/11, macOS, Linux
- **Rust**: 1.70以上
- **MySQL**: 8.0以上
- **メモリ**: 4GB以上
- **ディスク**: 1GB以上

### 起動手順
```bash
# 1. リポジトリクローン
git clone https://github.com/Yuphy919/AssetManager.git
cd AssetManager

# 2. 依存関係インストール
cargo build

# 3. データベース準備
mysql -u root -p < schema.sql

# 4. アップロードディレクトリ作成
mkdir uploads

# 5. サーバー起動
cargo run

# 6. ブラウザアクセス
http://localhost:8080
```

### 設定情報
- **データベース接続**: `mysql://root:root_pass@localhost:3306/my_assets`
- **ポート番号**: `8080`
- **静的ファイルパス**: `static/`
- **アップロードパス**: `uploads/`

---

## 📈 今後の拡張予定

### Phase 2 機能
- [ ] ユーザー認証機能
- [ ] 複数ポートフォリオ管理
- [ ] 履歴管理機能
- [ ] データエクスポート機能

### Phase 3 機能
- [ ] リアルタイム株価連携
- [ ] アラート機能
- [ ] レポート自動生成
- [ ] モバイルアプリ対応

---

## 📝 変更履歴

| バージョン | 日付 | 変更内容 | 担当者 |
|------------|------|----------|--------|
| 1.0.0 | 2025-07-27 | 初回リリース | GitHub Copilot |

---

## 📞 サポート情報

### 開発環境
- **IDE**: Visual Studio Code
- **拡張機能**: rust-analyzer, REST Client
- **デバッグ**: ブラウザ開発者ツール

### トラブルシューティング
- **データベース接続エラー**: MySQL サービス起動確認
- **ファイルアップロードエラー**: アップロードディレクトリの権限確認
- **ポート使用中エラー**: 他のプロセスのポート使用確認

### 連絡先
- **開発者**: GitHub Copilot
- **リポジトリ**: https://github.com/Yuphy919/AssetManager
- **Issue報告**: GitHub Issues
