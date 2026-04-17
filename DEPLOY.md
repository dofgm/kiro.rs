# kiro-rs 部署文档

> Anthropic Claude API 兼容代理服务 — 将 Anthropic API 请求转换为 Kiro API 请求

---

## 目录

- [架构概览](#架构概览)
- [环境要求](#环境要求)
- [方式一：源码编译部署](#方式一源码编译部署)
- [方式二：Docker 部署](#方式二docker-部署)
- [配置说明](#配置说明)
  - [config.json](#configjson)
  - [credentials.json](#credentialsjson)
- [启动与验证](#启动与验证)
- [接入 NewAPI / One API](#接入-newapi--one-api)
- [接入 Claude Code](#接入-claude-code)
- [自定义功能说明](#自定义功能说明)
- [运维与排障](#运维与排障)
- [常见问题](#常见问题)

---

## 架构概览

```
客户端 (Claude Code / NewAPI / curl)
  │
  │  Anthropic API 格式
  ▼
┌──────────────────────────────┐
│         kiro-rs 代理          │
│  ┌────────────────────────┐  │
│  │ 认证中间件 (API Key)    │  │
│  │ 请求转换 (Anthropic→Kiro)│ │
│  │ KV Cache 模拟          │  │
│  │ Billing Header 清洗    │  │
│  │ 响应转换 (Kiro→Anthropic)│ │
│  │ Tool Name 反映射       │  │
│  │ Thinking 块提取        │  │
│  └────────────────────────┘  │
│  监听: 127.0.0.1:8990       │
└──────────────────────────────┘
  │
  │  Kiro API 格式 (AWS Event Stream)
  ▼
┌──────────────────────────────┐
│     Kiro API (AWS 后端)       │
│     → Anthropic Claude       │
└──────────────────────────────┘
```

---

## 环境要求

### 源码编译

| 依赖 | 版本 | 说明 |
|------|------|------|
| Rust | 1.92+ | Edition 2024 |
| Node.js | 18+ | 构建 Admin UI |
| pnpm | 9+ | 前端包管理器 |

### Docker

| 依赖 | 版本 |
|------|------|
| Docker | 20+ |
| Docker Compose | v2+ |

---

## 方式一：源码编译部署

### 1. 克隆项目

```bash
git clone <repo-url> kiro-rs
cd kiro-rs
```

### 2. 构建 Admin UI（必须先于 Rust 编译）

```bash
cd admin-ui && pnpm install && pnpm build && cd ..
```

> Admin UI 的构建产物 (`admin-ui/dist/`) 会通过 `rust-embed` 嵌入到最终二进制中。

### 3. 编译 Rust 项目

```bash
cargo build --release
```

编译产物位于 `target/release/kiro-rs`。

### 4. 准备配置文件

```bash
cp config.example.json config.json
# 编辑 config.json，参见下方配置说明
```

```bash
cp credentials.example.social.json credentials.json
# 编辑 credentials.json，填入你的 Kiro 凭据
```

### 5. 启动

```bash
./target/release/kiro-rs
```

或指定配置路径：

```bash
./target/release/kiro-rs -c /path/to/config.json --credentials /path/to/credentials.json
```

---

## 方式二：Docker 部署

### 1. 准备配置目录

```bash
mkdir -p config
cp config.example.json config/config.json
cp credentials.example.social.json config/credentials.json
# 编辑两个配置文件
```

### 2. 使用 docker-compose 启动

```bash
docker-compose up -d
```

`docker-compose.yml` 默认映射端口 `8990:8990`，配置文件挂载到 `/app/config/`。

### 3. 自行构建镜像

```bash
docker build -t kiro-rs:latest .
docker run -d --name kiro-rs \
  -p 8990:8990 \
  -v $(pwd)/config:/app/config \
  kiro-rs:latest
```

---

## 配置说明

### config.json

**最小配置：**

```json
{
  "host": "0.0.0.0",
  "port": 8990,
  "apiKey": "sk-your-secret-api-key",
  "region": "us-east-1"
}
```

**完整字段：**

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `host` | string | `127.0.0.1` | 监听地址。局域网访问需改为 `0.0.0.0` |
| `port` | number | `8080` | 监听端口 |
| `apiKey` | string | **必填** | 客户端认证用的 API Key |
| `region` | string | `us-east-1` | 默认 AWS 区域 |
| `authRegion` | string | - | Token 刷新区域（回退到 region） |
| `apiRegion` | string | - | API 请求区域（回退到 region） |
| `tlsBackend` | string | `rustls` | TLS 后端：`rustls` 或 `native-tls` |
| `proxyUrl` | string | - | 全局 HTTP/SOCKS5 代理 |
| `proxyUsername` | string | - | 代理用户名 |
| `proxyPassword` | string | - | 代理密码 |
| `adminApiKey` | string | - | Admin API 密钥，配置后启用管理界面 |
| `loadBalancingMode` | string | `priority` | `priority`（优先级）或 `balanced`（均衡） |
| `extractThinking` | boolean | `true` | 非流式响应自动提取 `<thinking>` 块 |
| `countTokensApiUrl` | string | - | 外部 count_tokens API 地址 |
| `countTokensApiKey` | string | - | 外部 count_tokens API 密钥 |
| `countTokensAuthType` | string | `x-api-key` | `x-api-key` 或 `bearer` |

### credentials.json

支持**单对象**（向后兼容）或**数组**（多凭据）两种格式。

#### Social 认证（推荐）

```json
{
  "refreshToken": "你的刷新token",
  "expiresAt": "2025-12-31T02:32:45.144Z",
  "authMethod": "social",
  "machineId": "64位十六进制字符串"
}
```

#### IdC / Builder-ID 认证

```json
{
  "refreshToken": "你的刷新token",
  "expiresAt": "2025-12-31T02:32:45.144Z",
  "authMethod": "idc",
  "clientId": "你的clientId",
  "clientSecret": "你的clientSecret",
  "region": "us-east-2"
}
```

#### API Key 认证

```json
{
  "kiroApiKey": "ksk_your_api_key_here",
  "authMethod": "api_key"
}
```

#### 多凭据（负载均衡 + 故障转移）

```json
[
  {
    "refreshToken": "主凭据token",
    "authMethod": "social",
    "priority": 0
  },
  {
    "refreshToken": "备用凭据token",
    "authMethod": "social",
    "priority": 1
  }
]
```

- `priority` 数字越小越优先
- 单凭据最多重试 3 次，单请求最多重试 9 次
- 多凭据格式下 Token 刷新后自动回写到文件

#### 凭据字段速查

| 字段 | 说明 |
|------|------|
| `refreshToken` | OAuth 刷新令牌 |
| `expiresAt` | Token 过期时间 (RFC3339) |
| `authMethod` | `social` / `idc` / `api_key` |
| `clientId` / `clientSecret` | IdC 认证必填 |
| `priority` | 优先级，默认 0 |
| `region` / `authRegion` / `apiRegion` | 凭据级区域覆盖 |
| `machineId` | 凭据级机器码 |
| `proxyUrl` | 凭据级代理（`direct` = 不走代理） |
| `disabled` | 设为 `true` 禁用该凭据 |

---

## 启动与验证

### 启动服务

```bash
# 前台运行
./target/release/kiro-rs

# 后台运行
nohup ./target/release/kiro-rs > kiro-rs.log 2>&1 &

# 指定日志级别
RUST_LOG=info ./target/release/kiro-rs
RUST_LOG=debug ./target/release/kiro-rs  # 详细调试
```

### 验证服务可用

```bash
# 检查模型列表
curl http://127.0.0.1:8990/v1/models \
  -H "x-api-key: sk-your-api-key"

# 发送测试请求
curl http://127.0.0.1:8990/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: sk-your-api-key" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 32,
    "stream": false,
    "messages": [{"role": "user", "content": "Hi"}]
  }'
```

### 验证缓存功能

发送相同请求两次，观察 usage 中的缓存字段：
- 第一次：`cache_creation_input_tokens > 0`，`cache_read_input_tokens = 0`
- 第二次：`cache_creation_input_tokens = 0`，`cache_read_input_tokens > 0`

> 注意：输入 token 数 < 1000 的小请求不参与缓存模拟。

---

## 接入 NewAPI / One API

### 渠道配置

| 配置项 | 值 |
|--------|-----|
| 类型 | Anthropic Claude |
| Base URL | `http://127.0.0.1:8990` |
| API Key | config.json 中的 `apiKey` 值 |
| 模型 | 手动添加（见下方） |

### 可用模型

| 模型名 | 说明 |
|--------|------|
| `claude-opus-4-6` | Opus 4.6 |
| `claude-opus-4-6-thinking` | Opus 4.6 + Thinking (自动开启 adaptive) |
| `claude-sonnet-4-6` | Sonnet 4.6 |
| `claude-sonnet-4-6-thinking` | Sonnet 4.6 + Thinking |
| `claude-haiku-4-5-20251001` | Haiku 4.5 |

> 带 `-thinking` 后缀的模型会自动覆写 thinking 配置为 `adaptive` 模式，无需客户端手动设置。

### 注意事项

- NewAPI 模型测试使用**非流式**请求，速度会比实际使用慢（~15-25s），这是 Kiro 上游延迟，属正常现象
- 实际流式使用体验 TTFB 约 1-2s
- NewAPI 前端可能不显示 `cache_creation_input_tokens` / `cache_read_input_tokens` 字段，这是 NewAPI 的显示限制，API 响应中已包含

---

## 接入 Claude Code

### 配置环境变量

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:8990
export ANTHROPIC_API_KEY=sk-your-api-key
```

### Claude Code 专用端点

对于需要精确 `input_tokens` 的场景，使用 `/cc/v1` 端点：

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:8990/cc
```

**`/cc/v1/messages` 与 `/v1/messages` 的区别：**
- `/v1/messages`：实时流式返回，`input_tokens` 为估算值
- `/cc/v1/messages`：缓冲模式，等待上游流完成后用精确的 `input_tokens` 更正响应，每 25s 发送 `ping` 保活

---

## 自定义功能说明

本 fork 在上游基础上增加了以下功能：

### 1. KV Cache 模拟 (Prompt Cache)

由于 Kiro 上游不返回 prompt cache 信息，本服务在本地模拟 KV Cache 行为：

- 将 system / tools / messages 归一化后生成 prompt block hash
- 在本地维护历史 prompt 记录（按 endpoint + model 分组）
- 对新请求，寻找"最长前缀匹配"作为 cache read
- 响应中注入 `cache_creation_input_tokens` 和 `cache_read_input_tokens`
- 阈值：输入 token < 1000 时不参与缓存
- 记录文件：`kiro_kv_cache_records.jsonl`

### 2. Billing Header 清洗

自动移除 system prompt 中的 `x-anthropic-billing-header` 标签，防止被 Kiro 上游拒绝。日志中会打印 `已预清洗 system 中的 x-anthropic-billing-header`。

### 3. Failure Prompt 日志

当请求失败时（工具调用错误、malformed request 等），自动记录失败请求的 prompt 到 `kiro_prompt_failure_records.jsonl`，方便排障。

### 4. Tool Name 反映射

Kiro 上游对超过 63 字符的工具名进行 SHA256 截断。本服务在响应中自动将截断的工具名映射回原始名称，对客户端透明。

### 5. Thinking 块提取

非流式响应中，自动将 `<thinking>...</thinking>` 标签解析为独立的 `thinking` 内容块，由 `extractThinking` 配置控制（默认开启）。

### 6. 动态 Context Window

- Claude 4.6 模型：1,000,000 tokens
- 其他模型：200,000 tokens

---

## 运维与排障

### 日志管理

```bash
# 标准日志
RUST_LOG=info ./target/release/kiro-rs 2>&1 | tee kiro-rs.log

# 调试模式（详细请求/响应日志）
RUST_LOG=debug ./target/release/kiro-rs
```

### 生成的文件

| 文件 | 说明 |
|------|------|
| `kiro_kv_cache_records.jsonl` | KV Cache 模拟记录 |
| `kiro_prompt_failure_records.jsonl` | 失败请求记录 |
| `kiro_stats.json` | 统计信息 |
| `kiro_balance_cache.json` | 余额缓存 |

### Admin 管理界面

配置 `adminApiKey` 后访问 `http://127.0.0.1:8990/admin`：

- 查看凭据状态、订阅等级
- 添加/删除/禁用凭据
- 查看余额
- 重置失败计数

### systemd 服务（Linux 生产部署）

```ini
# /etc/systemd/system/kiro-rs.service
[Unit]
Description=kiro-rs Anthropic API Proxy
After=network.target

[Service]
Type=simple
User=kiro
WorkingDirectory=/opt/kiro-rs
ExecStart=/opt/kiro-rs/kiro-rs -c /opt/kiro-rs/config.json --credentials /opt/kiro-rs/credentials.json
Environment=RUST_LOG=info
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now kiro-rs
sudo systemctl status kiro-rs
sudo journalctl -u kiro-rs -f  # 查看日志
```

---

## 常见问题

### Q: 请求速度慢（15-25s）？

这是 Kiro 上游的固有延迟，请求链路为 `客户端 → kiro-rs → Kiro API (AWS) → Claude`。kiro-rs 本地处理 < 1ms。使用流式模式可显著改善体感速度（TTFB ~1-2s）。

### Q: Token 刷新失败？

- 尝试将 `tlsBackend` 切换为 `native-tls`
- 检查网络是否需要配置代理 (`proxyUrl`)
- 确认 `refreshToken` 是否过期

### Q: 编译报错？

确保先构建了 Admin UI：
```bash
cd admin-ui && pnpm install && pnpm build && cd ..
```

### Q: NewAPI 不显示缓存信息？

API 响应中已包含 `cache_creation_input_tokens` / `cache_read_input_tokens`，但 NewAPI 前端不解析这些字段。用 curl 直接请求可以看到。这是 NewAPI 的显示限制。

### Q: Write Failed / 会话卡死？

参考 [Issue #22](https://github.com/hank9999/kiro.rs/issues/22) 和 [#49](https://github.com/hank9999/kiro.rs/issues/49)。通常与输出过长被截断有关，可尝试调低 max_tokens。

### Q: 如何获取 Kiro 凭据？

从 Kiro IDE 的认证流程中获取 `refreshToken` 等信息，或通过 Admin UI 添加凭据。

---

## API 端点速查

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/models` | GET | 模型列表 |
| `/v1/messages` | POST | 创建消息（流式/非流式） |
| `/v1/messages/count_tokens` | POST | Token 计数 |
| `/cc/v1/messages` | POST | 创建消息（缓冲模式，精确 input_tokens） |
| `/cc/v1/messages/count_tokens` | POST | Token 计数 |
| `/admin` | GET | 管理界面 |
| `/api/admin/credentials` | GET/POST | 凭据管理 |
| `/api/admin/credentials/:id/balance` | GET | 余额查询 |

客户端认证方式（二选一）：
```
x-api-key: sk-your-api-key
Authorization: Bearer sk-your-api-key
```
