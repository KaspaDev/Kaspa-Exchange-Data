# Git API Proxy

A self-hosted, read-only API service that proxies GitHub content with added features like **caching**, **load balancing**, **security isolation**, and **monthly aggregation**. Designed for high-performance usage by the `Kaspa-Exchange-Data` project.

## Architecture

The system is containerized and composed of three main layers:

1.  **Envoy Proxy** (Port 8080): The secure entry point. Handles rate limiting and load balancing.
2.  **API Cluster** (Internal): 3 Replicas of the Rust API service.
3.  **Caching Layer** (Port 6380): DragonflyDB (Redis-compatible) for caching responses.

See [architecture.md](architecture.md) for detailed diagrams.

## Features

- **Secure Access**: Port 3010 is closed. Access is only via Envoy (Port 8080).
- **High Performance**: 
    - **Caching**: Responses are cached for 5 minutes in DragonflyDB (`<10ms` latency).
    - **Aggregation**: Fetch an entire month's data in one request.
    - **Load Balancing**: Traffic is distributed across 3 API instances.
- **Safety**: Rate limited to 500 requests/minute to prevent abuse.
- **Repository Scoping**: Strictly restricted to a specific owner/repository.

## Installation & Setup

### Prerequisites

- Docker & Docker Compose
- *(Optional)* A GitHub Personal Access Token for higher rate limits

> **⚠️ Rate Limits:**
> - **Without `GITHUB_TOKEN`**: 60 requests/hour (unauthenticated - works for public repos)
> - **With `GITHUB_TOKEN`**: 5,000 requests/hour (authenticated - recommended for production)
>
> The API works without a token for public repositories, but you'll hit the lower rate limit quickly.

### Configuration

Create a `.env` file in the root directory:

```bash
GITHUB_USER=KaspaDev
GITHUB_REPO=Kaspa-Exchange-Data
# GITHUB_TOKEN is optional:
# - Without it: 60 requests/hour (works for public repos)
# - With it: 5,000 requests/hour (recommended for production)
GITHUB_TOKEN=your_github_token_here
```

### Running the Service

**Production Mode** (Core Services Only):
```bash
./run-prod.sh
```

**Development Mode** (Includes SonarQube):
```bash
./run-dev.sh
```

The API will be available at `http://localhost:8080`.
SonarQube (Dev only) will be at `http://localhost:9000`.

## API Usage

**Base URL**: `http://localhost:8080/api`

### Endpoints

#### 1. Read Index (List Directories)
List available tokens:
```bash
curl http://localhost:8080/api/github/KaspaDev/Kaspa-Exchange-Data/data
```

#### 2. Read Specific Data File
Fetch specific file:
```bash
curl http://localhost:8080/api/github/KaspaDev/Kaspa-Exchange-Data/data/slow/ascendex/2025/12/2025-12-28.json
```

#### 3. Aggregation (Synthetic Ranges)
Fetch an entire month's data in one request:
```bash
curl "http://localhost:8080/api/github/KaspaDev/Kaspa-Exchange-Data/data/slow/ascendex/2025/12?aggregate=true"
```

### Advanced Features

#### Caching
Responses are automatically cached.
- **Header**: Check `X-Cache: HIT` or `MISS`.
- **TTL**: 300 seconds (5 minutes).

#### Rate Limiting
- **Limit**: 500 requests per minute.
- **Response**: `429 Too Many Requests` if exceeded.

## License
[MIT](LICENSE)
