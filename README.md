<p align="center">
  <h1 align="center">kiro-rs</h1>
  <p align="center">
    用 Rust 编写的 Anthropic Claude API 兼容代理服务
    <br />
    将 Anthropic API 请求转换为 Kiro API 请求
  </p>
</p>

<p align="center">
  <a href="#功能特性">功能特性</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#配置详解">配置详解</a> •
  <a href="#api-端点">API 端点</a> •
  <a href="#admin-管理">Admin 管理</a>
</p>

---

> **免责声明**：本项目仅供研究使用，Use at your own risk。使用本项目所导致的任何后果由使用人承担，与本项目无关。本项目与 AWS / KIRO / Anthropic / Claude 等官方无关，不代表官方立场。

---

## 功能特性

| 分类 | 特性 |
|------|------|
| **API 兼容** | 完整支持 Anthropic Claude API 格式，SSE 流式输出 |
| **模型支持** | Sonnet 4.5/4.6、Opus 4.5/4.6、Haiku 4.5 全系列 |
| **高级功能** | Extended Thinking、Tool Use / Function Calling、WebSearch |
| **凭据管理** | 多凭据支持、自动故障转移、Token 自动刷新与回写 |
| **负载均衡** | Priority / Balanced / Weighted Round Robin 三种模式 |
| **智能重试** | 单凭据最多 3 次，单请求最多 9 次 |
| **代理支持** | 全局代理 + 凭据级代理，HTTP / SOCKS5 |
| **Admin** | Web 管理界面 + REST API，凭据管理、余额查询、API Key 管理 |
| **多级 Region** | 全局和凭据级别的 Auth Region / API Region 配置 |

## 快速开始

### 1. 编译

> 如果不想编译，可以直接前往 [Release](https://github.com/Zhang161215/kiro.rs/releases) 下载二进制文件

```bash
# 先构建前端 Admin UI（嵌入到二进制中）
cd admin-ui && pnpm install && pnpm build && cd ..

# 编译
cargo build --release
```

### 2. 最小配置

创建 `config.json`：

```json
{
   "host": "127.0.0.1",
   "port": 8990,
   "apiKey": "sk-kiro-rs-your-secret-key",
   "region": "us-east-1",
   "adminApiKey": "sk-admin-your-secret-key"
}
```

创建 `credentials.json`（从 Kiro IDE 中获取凭证信息，也可通过 Web 管理面板添加）：

<details>
<summary>Social 认证</summary>

```json
{
   "refreshToken": "你的刷新token",
   "expiresAt": "2025-12-31T02:32:45.144Z",
   "authMethod": "social"
}
```
</details>

<details>
<summary>IdC 认证</summary>

```json
{
   "refreshToken": "你的刷新token",
   "expiresAt": "2025-12-31T02:32:45.144Z",
   "authMethod": "idc",
   "clientId": "你的clientId",
   "clientSecret": "你的clientSecret"
}
```
</details>

### 3. 启动

```bash
./target/release/kiro-rs
```

或指定配置文件路径：

```bash
./target/release/kiro-rs -c /path/to/config.json --credentials /path/to/credentials.json
```

### 4. 验证

```bash
curl http://127.0.0.1:8990/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: sk-kiro-rs-your-secret-key" \
  -d '{
    "model": "claude-sonnet-4-5-20250929",
    "max_tokens": 1024,
    "stream": true,
    "messages": [{"role": "user", "content": "Hello, Claude!"}]
  }'
```

### Docker

```bash
docker-compose up
```

需要将 `config.json` 和 `credentials.json` 挂载到容器中，具体参见 `docker-compose.yml`。

## 配置详解

### config.json

| 字段 | 类型 | 默认值 | 描述 |
|------|------|--------|------|
| `host` | string | `127.0.0.1` | 服务监听地址 |
| `port` | number | `8080` | 服务监听端口 |
| `apiKey` | string | - | 自定义 API Key（客户端认证，必配） |
| `region` | string | `us-east-1` | AWS 区域 |
| `authRegion` | string | - | Auth Region（Token 刷新），未配置时回退到 region |
| `apiRegion` | string | - | API Region（API 请求），未配置时回退到 region |
| `kiroVersion` | string | `0.10.0` | Kiro 版本号 |
| `machineId` | string | - | 自定义机器码（64位十六进制），不定义则自动生成 |
| `tlsBackend` | string | `rustls` | TLS 后端：`rustls` 或 `native-tls` |
| `proxyUrl` | string | - | HTTP/SOCKS5 代理地址 |
| `proxyUsername` | string | - | 代理用户名 |
| `proxyPassword` | string | - | 代理密码 |
| `adminApiKey` | string | - | Admin API 密钥，配置后启用管理功能 |
| `loadBalancingMode` | string | `priority` | 负载均衡：`priority` / `balanced` / `weighted_round_robin` |
| `stripBillingHeader` | bool | `true` | 是否移除请求中的 billing header |
| `countTokensApiUrl` | string | - | 外部 count_tokens API 地址 |
| `countTokensApiKey` | string | - | 外部 count_tokens API 密钥 |

<details>
<summary>完整配置示例</summary>

```json
{
   "host": "127.0.0.1",
   "port": 8990,
   "apiKey": "sk-kiro-rs-your-secret-key",
   "region": "us-east-1",
   "authRegion": "us-east-1",
   "apiRegion": "us-east-1",
   "tlsBackend": "rustls",
   "proxyUrl": "http://127.0.0.1:7890",
   "adminApiKey": "sk-admin-your-secret-key",
   "loadBalancingMode": "priority",
   "stripBillingHeader": true
}
```
</details>

### credentials.json

支持单对象格式（向后兼容）或数组格式（多凭据）。

| 字段 | 类型 | 描述 |
|------|------|------|
| `refreshToken` | string | OAuth 刷新令牌 |
| `expiresAt` | string | Token 过期时间 (RFC3339) |
| `authMethod` | string | 认证方式：`social` 或 `idc` |
| `clientId` | string | IdC 认证的客户端 ID |
| `clientSecret` | string | IdC 认证的客户端密钥 |
| `priority` | number | 优先级，数字越小越优先，默认 0 |
| `region` | string | 凭据级 Region |
| `authRegion` | string | 凭据级 Auth Region |
| `apiRegion` | string | 凭据级 API Region |
| `machineId` | string | 凭据级机器码 |
| `proxyUrl` | string | 凭据级代理 URL（`direct` 表示不使用代理） |

<details>
<summary>多凭据格式示例</summary>

```json
[
   {
      "refreshToken": "第一个凭据",
      "authMethod": "social",
      "priority": 0
   },
   {
      "refreshToken": "第二个凭据（IdC + 独立代理）",
      "authMethod": "idc",
      "clientId": "xxx",
      "clientSecret": "xxx",
      "region": "us-east-2",
      "priority": 1,
      "proxyUrl": "socks5://proxy.example.com:1080"
   },
   {
      "refreshToken": "第三个凭据（显式直连）",
      "authMethod": "social",
      "priority": 2,
      "proxyUrl": "direct"
   }
]
```
</details>

### Region 配置

**Auth Region**（Token 刷新）优先级：
`凭据.authRegion` > `凭据.region` > `config.authRegion` > `config.region`

**API Region**（API 请求）优先级：
`凭据.apiRegion` > `config.apiRegion` > `config.region`

### 代理配置

**优先级**：`凭据.proxyUrl` > `config.proxyUrl` > 无代理

| 凭据 `proxyUrl` 值 | 行为 |
|---|---|
| 具体 URL | 使用凭据指定的代理 |
| `direct` | 显式不使用代理 |
| 未配置 | 回退到全局代理 |

### 认证方式

客户端请求支持两种认证方式：

```
x-api-key: sk-your-api-key
```
```
Authorization: Bearer sk-your-api-key
```

## API 端点

### 标准端点 (/v1)

| 端点 | 方法 | 描述 |
|------|------|------|
| `/v1/models` | GET | 获取可用模型列表 |
| `/v1/messages` | POST | 创建消息（对话） |
| `/v1/messages/count_tokens` | POST | 估算 Token 数量 |

### Claude Code 兼容端点 (/cc/v1)

| 端点 | 方法 | 描述 |
|------|------|------|
| `/cc/v1/messages` | POST | 创建消息（缓冲模式，确保 `input_tokens` 准确） |
| `/cc/v1/messages/count_tokens` | POST | 估算 Token 数量 |

> `/cc/v1/messages` 会等待上游流完成后，用 `contextUsageEvent` 中的准确 `input_tokens` 更正 `message_start`，等待期间每 25 秒发送 `ping` 保活。

### Thinking 模式

```json
{
  "model": "claude-sonnet-4-5-20250929",
  "max_tokens": 16000,
  "thinking": { "type": "enabled", "budget_tokens": 10000 },
  "messages": [{"role": "user", "content": "..."}]
}
```

### 工具调用

完整支持 Anthropic 的 Tool Use / Function Calling。

## 模型映射

| Anthropic 模型 | Kiro 模型 |
|----------------|-----------|
| `*sonnet*` | `claude-sonnet-4.5` |
| `*opus*`（含 4.5/4-5） | `claude-opus-4.5` |
| `*opus*`（其他） | `claude-opus-4.6` |
| `*haiku*` | `claude-haiku-4.5` |

## Admin 管理

当 `config.json` 配置了 `adminApiKey` 时，启用 Web 管理界面和 REST API。

### Web 管理界面

访问 `http://localhost:8990/admin`，使用 `adminApiKey` 登录。

功能包括：
- 凭据管理（添加、删除、启用/禁用、批量导入、批量验活）
- 余额查询与订阅类型筛选（FREE / PRO / PRO+ / POWER）
- 请求明细查看（模型、Token 用量、缓存占比、花费）
- 设置面板（可用模型列表、API Key 管理、Billing Header 开关）
- 负载均衡模式切换
- 暗色模式

### Admin API

| 端点 | 方法 | 描述 |
|------|------|------|
| `/api/admin/credentials` | GET | 获取所有凭据状态 |
| `/api/admin/credentials` | POST | 添加新凭据 |
| `/api/admin/credentials/:id` | DELETE | 删除凭据 |
| `/api/admin/credentials/:id/disabled` | POST | 设置凭据禁用状态 |
| `/api/admin/credentials/:id/priority` | POST | 设置凭据优先级 |
| `/api/admin/credentials/:id/reset` | POST | 重置失败计数 |
| `/api/admin/credentials/:id/balance` | GET | 获取凭据余额 |
| `/api/admin/details` | GET | 获取请求明细 |
| `/api/admin/details` | DELETE | 清空请求明细 |
| `/api/admin/config/load-balancing` | GET/PUT | 获取/设置负载均衡模式 |
| `/api/admin/config/system-settings` | GET/PUT | 获取/设置系统设置 |
| `/api/admin/config/models` | GET | 获取可用模型列表 |
| `/api/admin/config/api-key` | GET/PUT | 获取/设置 API 密钥 |

## 注意事项

> **TLS 后端**：默认使用 `rustls`，如遇到请求报错（无法刷新 token、error request），尝试在 `config.json` 中将 `tlsBackend` 切换为 `native-tls`。

> **Write Failed / 会话卡死**：参考 Issue [#22](https://github.com/hank9999/kiro.rs/issues/22) 和 [#49](https://github.com/hank9999/kiro.rs/issues/49)，通常与输出过长被截断有关。

- 请妥善保管 `credentials.json`，不要提交到版本控制
- Token 会自动刷新，无需手动干预
- 当 `tools` 列表仅包含一个 `web_search` 工具时，会走内置 WebSearch 转换逻辑

## 项目结构

```
kiro-rs/
├── src/
│   ├── main.rs                  # 程序入口
│   ├── anthropic/               # Anthropic API 兼容层
│   │   ├── router.rs            # 路由配置
│   │   ├── handlers.rs          # 请求处理器
│   │   ├── middleware.rs        # 认证中间件
│   │   ├── converter.rs         # 协议转换器
│   │   ├── stream.rs            # 流式响应处理
│   │   └── websearch.rs         # WebSearch 工具处理
│   ├── kiro/                    # Kiro API 客户端
│   │   ├── provider.rs          # API 提供者
│   │   ├── token_manager.rs     # Token 管理（多凭据、负载均衡）
│   │   ├── model/               # 数据模型
│   │   └── parser/              # AWS Event Stream 解析器
│   ├── admin/                   # Admin API 模块
│   │   ├── router.rs            # 路由配置
│   │   ├── handlers.rs          # 请求处理器
│   │   ├── service.rs           # 业务逻辑
│   │   └── middleware.rs        # 认证中间件
│   ├── admin_ui/                # Admin UI 静态文件嵌入
│   └── common/                  # 公共模块
├── admin-ui/                    # Admin UI 前端（React + Tailwind）
├── Cargo.toml
├── Dockerfile
└── docker-compose.yml
```

## 技术栈

- **Web 框架**: [Axum](https://github.com/tokio-rs/axum) 0.8
- **异步运行时**: [Tokio](https://tokio.rs/)
- **HTTP 客户端**: [Reqwest](https://github.com/seanmonstar/reqwest)
- **序列化**: [Serde](https://serde.rs/)
- **前端**: React + TypeScript + Tailwind CSS + shadcn/ui
- **静态嵌入**: [rust-embed](https://github.com/pyrossh/rust-embed)

## License

MIT

## 致谢

本项目的实现离不开前辈的努力：
- [kiro2api](https://github.com/caidaoli/kiro2api)
- [proxycast](https://github.com/aiclientproxy/proxycast)

<table>
<tr>
<td>
<b>特别感谢</b>：感谢某 AI 服务商为本项目提供的 API 额度支持<br>
<sub>* 为遵守相关规则，已移除相关链接</sub>
</td>
</tr>
</table>
