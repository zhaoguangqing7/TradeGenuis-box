# TradeGenuis · 箱体突破战法看板 (Rust + PostgreSQL 版)

一个基于**公开行情接口、全自动、高并发**的箱体突破选股/选币看板与量化扫描系统。
将「箱体突破四条件」超短线战法以 **Rust 原生重构**，搭配 **PostgreSQL (PgSQL)** 实现结构化数据存储与历史回溯，同时提供完整的图形化 Web 看板。

---

## 🌟 核心特性

- 🦀 **Rust 异步高性能架构**：基于 `tokio` 异步运行时与 `axum` Web 框架，多线程高并发抓取与计算，毫秒级响应。
- 🐘 **PostgreSQL 结构化持久化**：
  - **自动初始化建表**（Auto-migration）：启动时自动创建表结构与索引，无需手动执行 DDL。
  - **平滑降级机制**：未配置或无法连接数据库时，系统自动切换至本地 JSON 缓存模式，开箱即用。
- 📊 **四条件量化打分体系**（满分 100 分，各 25 分，≥85 分达标）：
  1. **热点题材**：实时抓取东财概念板块当日涨幅 TOP 10 及自定义板块进行双向题材匹配。
  2. **倍量启动**：近 5 日均量 1.8 倍以上连续放量天数统计。
  3. **主力资金与高控盘**：近 5 日主力净流入天数 + 股东户数环比（筹码集中度）分析。
  4. **箱体上沿试盘**：自动识别 60 根日 K 线整理箱体，精准捕捉触顶逼近、放量、上影线等主力探顶试盘动作（$\ge 3$ 次）。
- 🌐 **双市场支持**：
  - **A 股市场**：东财 push2 + 腾讯行情 qt + 新浪金融多源主备容灾。
  - **加密货币**：Binance USDT 永续合约 24h 涨幅榜与日 K 线行情。
- 🖥️ **现代化深色图形看板**：内嵌蜡烛图 K 线、箱体虚线上下沿、四条件徽章、自选池管理及每 3 秒实时价格刷新。
- ⏰ **定时调度**：交易日 11:30（午盘）与 15:00（收盘）后台自动触发全量扫描。

---

## 🚀 快速启动

### 1. 环境准备
* 安装 **Rust** (推荐 1.75+): [https://rustup.rs/](https://rustup.rs/)
* (可选) **PostgreSQL 12+**

### 2. 配置数据库连接
复制 `.env.example` 为 `.env`：
```bash
cp .env.example .env
```
编辑 `.env` 设置你的 PostgreSQL 连接串：
```env
DATABASE_URL=postgres://postgres:password@localhost:5432/tradegenius
HOST=127.0.0.1
PORT=8808
```

### 3. 编译并运行

#### 方式 A：一键启动脚本
```bash
bash start.sh
```

#### 方式 B：Cargo 命令行
```bash
# 1. 编译并启动 Web 看板服务（默认 http://127.0.0.1:8808）
cargo run --release

# 2. 执行单次命令行扫描
cargo run --release -- --scan pool      # 扫描自选池股票
cargo run --release -- --scan crypto    # 扫描币圈合约
```

启动后在浏览器打开：👉 **http://127.0.0.1:8808**

---

## 🗄️ 数据库表结构设计 (PostgreSQL)

| 表名 | 说明 | 核心字段 |
|---|---|---|
| `pool` | 用户自选股票池 | `code`, `name`, `theme`, `created_at` |
| `scan_records` | 扫描批次汇总表 | `id`, `market`, `scan_type`, `total_scanned`, `total_qualified`, `hot_topics` |
| `scan_candidates` | 扫描结果与指标明细表 | `code`, `name`, `score`, `mode`, `qualified`, `box_low`, `box_high`, `tests`, `volume_days`, `fund_5d`, `control`, `flags` |
| `klines` | K 线历史与缓存 | `code`, `market`, `k_date`, `open`, `high`, `low`, `close`, `volume` |
| `system_config` | 系统运行配置 | `config_key`, `config_value` (JSONB) |

---

## 📡 RESTful API 接口清单

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/` | 看板 HTML 首页 |
| `GET` | `/api/watchlist` | 获取最近一次 A 股扫描结果 |
| `GET` | `/api/crypto` | 获取最近一次币圈扫描结果 |
| `GET` | `/api/hot` | 获取东财实时热点概念板块榜 |
| `GET` | `/api/pool` | 获取自选股票池 |
| `POST` | `/api/pool` | 动态添加/移除自选标的 |
| `GET` | `/api/quotes?codes=...` | 批量实时行情（价格/涨跌/量比/换手） |
| `GET` | `/api/kline?code=...` | 获取日 K 线及箱体试盘区间数据 |
| `POST` | `/api/scan` | 触发后台扫描任务 (`{ "mode": "pool" }`) |
| `GET` | `/api/status` | 获取扫描状态、后台日志及开盘交易状态 |
| `GET/POST`| `/api/config` | 查询与保存配置（自动扫描开关、定时时间等） |

---

## 📂 项目结构

```text
TradeGenuis-box/
├── Cargo.toml          # Rust 依赖与包配置
├── .env.example        # 环境变量配置文件模板
├── start.sh            # 一键启动脚本
├── dashboard.html      # 前端交互看板页面
├── static/             # 自托管字体与静态资源
│   └── fonts/
├── data/               # 本地数据文件与缓存（未连 DB 时的降级存储）
│   └── pool.json       # 自选股票池文件
└── src/                # Rust 核心源代码
    ├── main.rs         # 入口文件与 CLI 参数解析
    ├── models.rs       # 实体数据结构定义
    ├── db.rs           # PostgreSQL 连接池与 CRUD
    ├── crawler.rs      # 多源行情爬虫（东财/腾讯/新浪/Binance）
    ├── engine.rs       # 60日箱体计算、试盘算法与打分引擎
    ├── server.rs       # Axum Web 路由与 API 实现
    └── scheduler.rs    # 交易日后台自动定时调度
```

---

## ⚠️ 免责声明
本项目所采用的行情指标、箱体模型与试盘判定均为机械化量化规则近似，**仅供量化研究与技术学习交流使用，不构成任何投资建议**。
