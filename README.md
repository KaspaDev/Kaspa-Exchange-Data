# Kaspa Exchange Data Repository

This repository stores historical exchange data for KRC20 tokens using Kaspa Exchange Data API integration.

## Overview

This repository contains:
- **Raw ticker data**: Minute-by-minute exchange data for all configured tokens
- **Daily summaries**: OHLC (Open, High, Low, Close) aggregations per exchange
- **EOD summaries**: End-of-day summaries aggregated across all exchanges

## Repository Structure

```
Kaspa-Exchange-Data/
├── data/
│   ├── {token}/
│   │   ├── {exchange}/
│   │   │   ├── {year}/
│   │   │   │   ├── {month}/
│   │   │   │   │   ├── {date}.json          # Daily OHLC summary
│   │   │   │   │   └── {date}-raw.json     # Raw minute-by-minute data
│   │   │   │   └── ...
│   │   │   └── ...
│   │   └── ...
│   └── ...
└── summaries/
    └── daily/
        ├── {token}/
        │   ├── {date}-summary.json          # EOD summary
        │   └── latest.json                  # Latest summary
        └── ...
```

## Data Format

### Raw Data File
```json
{
  "timeframe": "24h",
  "token": "kaspa",
  "exchange": "ascendex",
  "resolution": "1m",
  "count": 132,
  "data": [
    {
      "timestamp": 1766860574,
      "last": 0.04477,
      "bid": 0.0445,
      "ask": 0.04484,
      "high": 0.04512,
      "low": 0.04214,
      "volume": null,
      "quoteVolume": 53620.27,
      "change": 0,
      "percentage": 0
    }
  ]
}
```

### Daily Summary File
```json
{
  "date": "2025-12-28",
  "token": "kaspa",
  "exchange": "ascendex",
  "open": 0.04477,
  "high": 0.04512,
  "low": 0.04214,
  "close": 0.04475,
  "volume": 1234567.89,
  "quoteVolume": 55123.45,
  "change": -0.00002,
  "percentage": -0.0447,
  "trades": 1320,
  "avgPrice": 0.04485,
  "vwap": 0.04482
}
```

## Accessing Data

### Via Kaspa Exchange Data API
```
http://localhost:8080/data/kaspa/ascendex/2025/12/2025-12-28.json
```

### Via Frontend API
```
GET /api/history/kaspa/ascendex?timeframe=24h
GET /api/history/kaspa/ascendex?timeframe=7d
GET /api/history/kaspa/ascendex?timeframe=30d
```

## Supported Tokens

- **Kaspa** (KAS) - 119+ exchanges
- **Nacho** - Multiple exchanges
- **Slow** - Multiple exchanges
- **TBDai** - Multiple exchanges
- **Zeal** - MEXC, CoinEx
- And many more KRC20 tokens...

## Data Updates

- **Continuous Updates**: Data is committed every 5 minutes
- **Daily Summaries**: Generated at end of day (EOD)
- **Git Tags**: Created daily with format `eod-YYYY-MM-DD`

## Version Control

All data is version-controlled in Git:
- Full commit history for all changes
- Daily tags for easy navigation
- Complete audit trail

## Privacy

This is a **public repository** containing exchange market data.

## License

MIT License - All rights reserved.

## Contact

Repository maintained by KaspaDev for exchange data storage and analysis.