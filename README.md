# 📊 Kaspa Exchange Data API

> Real-time and historical exchange data for Kaspa (KAS) and KRC20 tokens across 100+ exchanges.

[![API Status](https://img.shields.io/badge/API-Live-brightgreen)](http://localhost:8080/health)
[![Swagger](https://img.shields.io/badge/Docs-Swagger%20UI-orange)](http://localhost:8080/swagger-ui)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## ✨ Features

- 🚀 **Fast**: Redis caching with sub-second response times
- 📈 **Real-time**: Data updated every 5 minutes from 100+ exchanges
- 🔧 **Simple API**: Get token stats with one request
- 📊 **Chart-ready**: OHLCV history for trading charts
- 📖 **Well-documented**: Interactive Swagger UI

---

## 🚀 Quick Start

### 1. Get Token Stats
```bash
# Current stats for Kaspa across all exchanges
curl http://localhost:8080/v1/ticker/kaspa

# With 7-day lookback
curl http://localhost:8080/v1/ticker/kaspa?range=7d
```

### 2. Get Chart Data
```bash
# Hourly OHLCV data for charting (7-day, 1h resolution)
curl "http://localhost:8080/v1/ticker/kaspa/history?range=7d&resolution=1h"
```

### 3. Explore the API
Open the interactive docs at: **http://localhost:8080/swagger-ui**

---

## 📡 API Reference

### Ticker API *(Recommended)*

Simple endpoints for aggregated token data:

| Endpoint | Description |
|----------|-------------|
| `GET /v1/ticker/{token}` | Current stats across all exchanges |
| `GET /v1/ticker/{token}/history` | OHLCV data for charting |

**Query Parameters:**

| Parameter | Values | Default | Description |
|-----------|--------|---------|-------------|
| `range` | `today`, `7d`, `30d` | `today` / `7d` | Lookback period |
| `resolution` | `1m`, `5m`, `1h`, `1d` | `1h` | Chart resolution (history only) |

**Example Response:**
```json
{
  "token": "kaspa",
  "timestamp": "2025-12-30T05:00:00Z",
  "range": "today",
  "exchanges": [
    {
      "exchange": "ascendex",
      "last": 0.04512,
      "high": 0.04561,
      "low": 0.04381,
      "volume_24h": 60334.82,
      "change_pct": 0.38
    }
  ],
  "aggregate": {
    "avg_price": 0.0453,
    "total_volume_24h": 7931946.18,
    "vwap": 0.0451,
    "exchange_count": 24
  }
}
```

---

### Content API *(Advanced)*

Direct access to raw repository data files:

```bash
# List all tokens
GET /v1/api/github/KaspaDev/Kaspa-Exchange-Data/data

# List exchanges for a token
GET /v1/api/github/KaspaDev/Kaspa-Exchange-Data/data/kaspa

# Get specific data file
GET /v1/api/github/KaspaDev/Kaspa-Exchange-Data/data/kaspa/ascendex/2025/12/2025-12-28.json

# Aggregate with pagination
GET /v1/api/github/.../data/kaspa/ascendex/2025/12?aggregate=true&limit=30
```

---

## 🏗️ Self-Hosting

> **⚠️ GitHub API Rate Limits:**
> - **Without `GITHUB_TOKEN`**: 60 requests/hour (unauthenticated - works for public repos)
> - **With `GITHUB_TOKEN`**: 5,000 requests/hour (authenticated - recommended for production)
>
> The API works without a token for public repositories, but you'll hit the lower rate limit quickly under load.

### Prerequisites
- Docker & Docker Compose
- *(Optional)* GitHub Personal Access Token (see rate limits above)

### Setup

1. **Clone the repository**
   ```bash
   git clone https://github.com/KaspaDev/Kaspa-Exchange-Data.git
   cd Kaspa-Exchange-Data
   ```

2. **Configure environment** (optional)
   ```bash
   cp .env.sample .env
   # Optionally add GITHUB_TOKEN for higher rate limits (see below)
   ```

3. **Start the services**
   ```bash
   docker compose up -d
   ```

4. **Verify**
   ```bash
   curl http://localhost:8080/health
   # {"status":"ok","version":"0.1.0",...}
   ```

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Envoy Proxy (:8080)                 │
│                    (Load Balancer)                      │
└────────────────────────┬────────────────────────────────┘
                         │
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
   ┌────────┐       ┌────────┐       ┌────────┐
   │ API #1 │       │ API #2 │       │ API #3 │
   │ :3010  │       │ :3010  │       │ :3010  │
   └────┬───┘       └────┬───┘       └────┬───┘
        │                │                │
        └────────────────┴────────────────┘
                         │
                    ┌────┴────┐
                    │ Dragonfly│
                    │ (Redis)  │
                    │  :6379   │
                    └──────────┘
```

---

## 📁 Data Format

### Repository Structure
```
data/
└── {token}/
    └── {exchange}/
        └── {year}/
            └── {month}/
                ├── {date}.json       # Daily OHLC summary
                └── {date}-raw.json   # Minute-by-minute data
```

### Supported Tokens

| Token | Exchanges | Symbol |
|-------|-----------|--------|
| Kaspa | 100+ | KAS |
| Nacho | Multiple | NACHO |
| Slow | ascendex, fameex | SLOW |
| *...and more* | | |

---

## 🔧 Configuration

Configuration via `config.yaml`:

```yaml
server:
  host: "0.0.0.0"
  port: 3010
  allowed_origins: "*"

allowed_repos:
  - source: github
    owner: KaspaDev
    repo: Kaspa-Exchange-Data
```

Environment variables:
- `GITHUB_TOKEN` - GitHub Personal Access Token (**optional**)
  - **Without token**: 60 requests/hour (unauthenticated - works for public repos)
  - **With token**: 5,000 requests/hour (authenticated - recommended for production)
  - The API works without a token for public repositories, but you'll have a much lower rate limit
- `REDIS_URL` - Redis connection URL (default: `redis://dragonfly:6379`)
- `RUST_LOG` - Log level (default: `info`)

---

## 📝 License

MIT License - see [LICENSE](LICENSE) for details.

---

## 🤝 Contributing

Contributions welcome! Please read our [Code of Conduct](CODE_OF_CONDUCT.md) first.

---

<p align="center">
  <strong>Built with ❤️ for the Kaspa ecosystem</strong>
</p>