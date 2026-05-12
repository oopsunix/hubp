<div align="center">
<h1>hubp</h1>

<p>`hubp` is a high-performance, multi-protocol proxy service written in Rust. It provides seamless acceleration and proxying for popular developer resources including GitHub, Docker Registry, and Hugging Face. Designed with efficiency and security in mind, `hubp` leverages the asynchronous power of [Axum](https://github.com/tokio-rs/axum) and [Tokio](https://github.com/tokio-rs/tokio).
</p>

<p>
  <a href="https://mit-license.org/">
    <img src="https://img.shields.io/github/license/tldrw/Axon?style=flat" alt="License">
  </a>
  <a href="https://github.com/tldrw/Axon">
    <img src="https://img.shields.io/github/stars/tldrw/Axon?style=flat" alt="Stars">
  </a>
  <a href="https://github.com/tldrw/Axon">
    <img src="https://img.shields.io/github/forks/tldrw/Axon?style=flat" alt="Forks">
  </a>
  <a href="https://github.com/tldrw/Axon/releases">
    <img src="https://img.shields.io/github/v/release/tldrw/Axon?sort=semver" alt="Release">
  </a>
</p>

<div>

English ｜ [中文](README_CN.md)

</div>
</div>

---

## 🚀 Core Features

- **Multi-Source Proxying**: 
    - **GitHub**: Accelerate access to Raw files, Releases, Archives, and Git clones.
    - **Docker Registry**: Transparent proxy for `docker.io`, `ghcr.io`, `gcr.io`, `quay.io`, and `k8s.io`.
    - **Hugging Face**: Speed up model and dataset downloads.
- **High Performance**: Built with Rust for minimal overhead and high throughput.
- **Smart Caching**: Integrated caching for Docker manifests and authentication tokens to reduce upstream latency.
- **Security & Control**:
    - **GeoIP Admission**: Restrict access by country with automatic database updates.
    - **LAN Bypass**: Intelligent detection to skip GeoIP checks for local network requests.
    - **Rate Limiting**: IP-based request limiting to prevent abuse.
    - **Access Lists**: Global white/black lists for repository paths and IP addresses.
- **Operational Excellence**:
    - **Hot Reloading**: Configuration changes are applied automatically without service interruption.
    - **Low Footprint**: Optimized binary size and memory usage.

---

## 🛠️ Quick Start

### Using Docker (Recommended)

1. Create a `config.yaml` file (see [Configuration](#-configuration)).
2. Use the following `docker-compose.yml`:

```yaml
services:
  hubp:
    image: oopsunix/hubp:latest
    container_name: hubp
    restart: unless-stopped
    volumes:
      - ./config.yaml:/app/config.yaml
    ports:
      - "45000:45000"
```

3. Start the service:
```bash
docker-compose up -d
```

### From Source

Ensure you have the Rust toolchain installed.

```bash
git clone https://github.com/oopsunix/ghproxy.git
cd ghproxy
cargo build --release
./target/release/hubp
```

---

## ⚙️ Configuration

`hubp` is configured via `config.yaml`. Below is a comprehensive example:

```yaml
server:
  host: "0.0.0.0"
  port: 45000
  debug: false

# --- Access Control ---
access:
  proxy: "" # Upstream proxy (e.g., "http://127.0.0.1:7890")
  white_list: [] # Allowed keywords/repos
  black_list:
    - "baduser/*"
  
  # GeoIP Country Restriction
  geoip:
    enabled: false
    allowedCountries: ["CN"]
    databasePath: "GeoLite2-Country.mmdb"
    databaseUrl: "https://github.com/P3TERX/GeoLite.mmdb/raw/download/GeoLite2-Country.mmdb"
    databaseUpdateDays: 30

# --- Rate Limiting ---
request_limit:
  limit_rate: 1000       # Max requests per period
  periodHours: 3.0       # Time window in hours
  white_list:            # Trusted IPs that bypass rate limits
    - "127.0.0.1"
    - "192.168.1.0/24"
  black_list: []         # Banned IPs

# --- Docker Cache ---
docker:
  tokenCache:
    enabled: true
    defaultTTL: "20m"
```

---

## 📖 Usage Instructions

### GitHub Proxy
Prepend your proxy address to any GitHub URL:
- **Raw File**: `http://your-proxy:45000/https://raw.githubusercontent.com/user/repo/branch/file`
- **Release**: `http://your-proxy:45000/https://github.com/user/repo/releases/download/v1.0/asset.zip`
- **Git Clone**: `git clone http://your-proxy:45000/https://github.com/user/repo.git`

### Docker Registry Proxy
Configure your Docker daemon or use the proxy address directly:
- **Pull Image**: `docker pull your-proxy:45000/library/alpine` (Proxies to `docker.io`)
- **Other Registries**: Use as a prefix, e.g., `your-proxy:45000/ghcr.io/user/image`

### Hugging Face Proxy
- **Model Download**: `http://your-proxy:45000/https://huggingface.co/gpt2/resolve/main/config.json`

---

## 👩‍💻 Development Guide

### Project Structure
- `src/core.rs`: Configuration and global application state.
- `src/module/`: Implementation of different proxy protocols (GitHub, Docker, HF).
- `src/handler.rs`: Main request handling and middleware logic.
- `src/tasks.rs`: Background tasks for GeoIP updates and config reloading.

### Running Tests
```bash
cargo test
```

### Build Optimization
The release profile is optimized for binary size and performance:
```bash
cargo build --release
```

---

## 📜 License

This project is licensed under the [MIT License](LICENSE).

---

Built with ❤️ by [oopsunix](https://github.com/oopsunix)
