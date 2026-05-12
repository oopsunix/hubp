<div align="center">
<h1>hubp</h1>

<p> hubp 是一款使用 Rust 编写的高性能多协议代理服务。它为 GitHub、Docker Registry 和 Hugging Face 等常用开发者资源提供无缝的加速与代理功能。hubp 充分利用了 [Axum](https://github.com/tokio-rs/axum) 和 [Tokio](https://github.com/tokio-rs/tokio) 的异步特性，旨在提供极高的运行效率与安全性。
</p>

<p>
  <a href="https://www.apache.org/licenses/LICENSE-2.0">
    <img src="https://img.shields.io/github/license/oopsunix/hubp?style=flat" alt="License">
  </a>
  <a href="https://github.com/oopsunix/hubp">
    <img src="https://img.shields.io/github/stars/oopsunix/hubp?style=flat" alt="Stars">
  </a>
  <a href="https://github.com/oopsunix/hubp">
    <img src="https://img.shields.io/github/forks/oopsunix/hubp?style=flat" alt="Forks">
  </a>
  <a href="https://github.com/oopsunix/hubp/releases">
    <img src="https://img.shields.io/github/v/release/oopsunix/hubp?sort=semver" alt="Release">
  </a>
</p>

<div>

中文 ｜ [English](README.md)

</div>
</div>

---

## 🚀 核心特性

- **多源代理支持**: 
    - **GitHub**: 加速访问 Raw 文件、Release 资源、存档文件（Archive）以及 Git 克隆。
    - **Docker Registry**: 透明代理 `docker.io`、`ghcr.io`、`gcr.io`、`quay.io` 和 `registry.k8s.io`。
    - **Hugging Face**: 提升模型和数据集的下载速度。
- **高性能**: 基于 Rust 构建，极低的系统开销和极高的吞吐量。
- **智能缓存**: 内置 Docker Manifest 和认证 Token 缓存，显著降低上游延迟。
- **安全与准入控制**:
    - **GeoIP 准入**: 支持按国家/地区限制访问，并具备数据库自动更新功能。
    - **局域网绕过**: 智能识别并自动跳过对局域网内请求的地理位置校验。
    - **速率限制**: 基于 IP 的请求频率限制，防止服务滥用。
    - **访问列表**: 提供全局的仓库路径和 IP 地址黑白名单控制。
- **卓越的运维体验**:
    - **配置热重载**: 配置文件修改后自动生效，无需重启服务。
    - **极低占用**: 优化后的二进制体积和内存消耗。

---

## 🛠️ 快速开始

### 使用 Docker (推荐)

1. 创建 `config.yaml` 配置文件（参考 [配置说明](#-配置说明)）。
2. 使用以下 `docker-compose.yml` 模板：

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

3. 启动服务：
```bash
docker-compose up -d
```

### 源码编译

请确保您已安装 Rust 工具链。

```bash
git clone https://github.com/oopsunix/ghproxy.git
cd ghproxy
cargo build --release
./target/release/hubp
```

---

## ⚙️ 配置说明

`hubp` 通过 `config.yaml` 进行配置。

<details>
<summary>点击展开配置模板</summary>

```yaml
server:
  host: "0.0.0.0"
  port: 45000
  debug: false

# --- 访问控制 ---
access:
  proxy: "" # 上游代理 (例如: "http://127.0.0.1:7890" 或 "socks5://user:pass@127.0.0.1:1080")
  white_list: [] # 允许访问的关键字/仓库
  black_list:
    - "baduser/*"
  
  # GeoIP 国家限制
  geoip:
    enabled: false
    allowedCountries: ["CN"]
    databasePath: "GeoLite2-Country.mmdb"
    databaseUrl: "https://github.com/P3TERX/GeoLite.mmdb/raw/download/GeoLite2-Country.mmdb"
    databaseUpdateDays: 30

# --- 速率限制 ---
request_limit:
  limit_rate: 1000       # 每个周期内的最大请求数
  periodHours: 3.0       # 时间窗口（小时）
  white_list:            # 绕过速率限制的信任 IP
    - "127.0.0.1"
    - "192.168.1.0/24"
  black_list: []         # 封禁的 IP

# --- Docker 缓存 ---
docker:
  tokenCache:
    enabled: true
    defaultTTL: "20m"
```
</details>

---

## 📖 使用指南

### GitHub 代理
将代理地址添加到任何 GitHub URL 之前：

| 资源类型 | 原始 URL | 代理后 URL |
| :--- | :--- | :--- |
| **Raw 文件** | `https://raw.githubusercontent.com/user/repo/branch/file` | `https://your-domain.com/https://raw.githubusercontent.com/user/repo/branch/file` |
| **Release 附件** | `https://github.com/user/repo/releases/download/v1/asset.zip` | `https://your-domain.com/https://github.com/user/repo/releases/download/v1/asset.zip` |
| **Git 克隆** | `https://github.com/user/repo.git` | `https://your-domain.com/https://github.com/user/repo.git` |
| **存档文件** | `https://github.com/user/repo/archive/refs/heads/main.zip` | `https://your-domain.com/https://github.com/user/repo/archive/refs/heads/main.zip` |

**命令行示例**:
```bash
# Wget 下载
wget https://your-domain.com/https://github.com/user/repo/releases/download/v1/asset.zip

# Git 克隆
git clone https://your-domain.com/https://github.com/user/repo.git

# Go Get 加速
GOPROXY=https://goproxy.cn,direct go get -v github.com/user/repo
# 或全局替换 GitHub 地址
git config --global url."https://your-domain.com/https://github.com/".insteadOf "https://github.com/"
```

### Docker 镜像加速
`hubp` 支持多种 Docker 注册表的透明代理。

#### 1. 直接拉取 (作为前缀)
- **Docker Hub**: `docker pull your-domain.com/library/alpine`
- **GHCR**: `docker pull your-domain.com/ghcr.io/oopsunix/hubp:latest`
- **Quay**: `docker pull your-domain.com/quay.io/coreos/etcd:latest`
- **GCR**: `docker pull your-domain.com/gcr.io/google-containers/pause:latest`

#### 2. 配置为全局镜像源
将 `hubp` 配置为默认镜像加速器，编辑 `/etc/docker/daemon.json`：

```json
{
  "registry-mirrors": [
    "https://your-domain.com"
  ]
}
```
重启 Docker 服务：`sudo systemctl restart docker`。之后即可直接拉取：`docker pull alpine`。

### Hugging Face 代理
加速下载 Hugging Face 上的模型和数据集。

- **直接下载**: `https://your-domain.com/https://huggingface.co/gpt2/resolve/main/config.json`
- **Git 克隆**: `git clone https://your-domain.com/https://huggingface.co/gpt2`

**使用 `huggingface-cli`**:
```bash
# 设置环境变量
export HF_ENDPOINT=https://your-domain.com

# 下载模型
huggingface-cli download --resume-download gpt2
```

---

## 👩‍💻 开发指南

### 项目结构
- `src/core.rs`: 核心配置与全局状态管理。
- `src/module/`: 各类代理协议的实现（GitHub, Docker, HF）。
- `src/handler.rs`: 主请求处理器与中间件 logic。
- `src/tasks.rs`: 后台任务（GeoIP 更新、配置热重载等）。

### 运行测试
```bash
cargo test
```

### 构建优化
发布配置已针对体积和性能进行了优化：
```bash
cargo build --release
```

---

## 📜 开源协议

本项目基于 [Apache License 2.0](LICENSE) 协议开源。

---

Built with ❤️ by [oopsunix](https://github.com/oopsunix)
