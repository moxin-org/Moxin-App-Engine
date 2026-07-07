# MoFA Agent框架

[English](README.md) | [简体中文](README_cn.md)

<p align="center">
    <img src="docs/images/mofa-logo.png" width="30%"/>
</p>


<div align="center">
  <a href="https://crates.io/crates/mofa-sdk">
    <img src="https://img.shields.io/crates/v/mofa-sdk.svg" alt="crates.io"/>
  </a>
  <a href="https://pypi.org/project/mofa-core/">
    <img src="https://img.shields.io/pypi/v/mofa-core.svg" alt="PyPI 最新版本"/>
  </a>
  <a href="https://github.com/mofa-org/mofa/blob/main/LICENSE">
    <img src="https://img.shields.io/github/license/mofa-org/mofa" alt="许可证"/>
  </a>
  <a href="https://docs.rs/mofa-sdk">
    <img src="https://img.shields.io/badge/built_with-Rust-dca282.svg?logo=rust"  alt="docs"/>
  </a>
  <a href="https://github.com/mofa-org/mofa/stargazers">
    <img src="https://img.shields.io/github/stars/mofa-org/mofa" alt="GitHub 星标数"/>
  </a>
</div>

<h2 align="center">
  <a href="https://mofa.ai/">官网</a>
  |
  <a href="https://mofa.ai/docs/0overview/">快速入门</a>
  |
  <a href="https://github.com/mofa-org/mofa">GitHub</a>
  |
  <a href="https://hackathon.mofa.ai/">比赛</a>
  |
  <a href="https://discord.com/invite/hKJZzDMMm9">社区</a>
</h2>

<p align="center">
 <img src="https://img.shields.io/badge/性能-极致-red?style=for-the-badge" />
 <img src="https://img.shields.io/badge/扩展-无限-orange?style=for-the-badge" />
 <img src="https://img.shields.io/badge/语言-多端-yellow?style=for-the-badge" />
 <img src="https://img.shields.io/badge/运行时-可编程-green?style=for-the-badge" />
</p>

## 概述
MoFA (Modular Framework for Agents) 不是又一个智能体框架。
它是第一个实现"一次编写，多语言共享"的生产级智能体框架，专注于**极致性能、无限扩展性和运行时可编程性**。
通过革命性的架构设计，独创**双层插件系统**（编译时插件 + 运行时插件），实现了业界罕见的"性能与灵活性"完美平衡。

MoFA的突破：</br>
✅ Rust内核 + UniFFI：极致性能 + 多语言原生调用 </br>
✅ 双层插件：编译时高性能 + 运行时零部署修改 </br>
✅ 微内核架构：模块化，易扩展</br>
✅ 云原生：天生支持分布式和边缘计算</br>

## 为什么选择MoFA？
### **性能优势**

- 基于Rust 零成本抽象
- 内存安全
- 比Python生态框架性能提升显著

### **多语言支持**

- 通过UniFFI生成Python、Java、Go、Kotlin、Swift绑定
- 支持多种语言调用Rust核心逻辑
- 跨语言调用性能优于传统FFI方案

### **运行时可编程**

- 集成Rhai脚本引擎
- 支持热重载业务逻辑
- 支持运行时配置和规则调整
- 用户自定义扩展


### **双层插件架构**

- **编译时插件**: 极致性能，原生集成
- **运行时插件**: 动态加载，即时生效
- 支持插件热加载和版本管理

### **分布式数据流 (Dora)**

- 支持Dora-rs分布式运行时
- 跨进程/跨机器Agent通信
- 适合边缘计算场景

### **Actor并发模型 (Ractor)**

- Agent间隔离性好
- 消息驱动架构
- 支持高并发场景

## 核心架构

### 微内核 + 双层插件系统

MoFA采用**分层微内核架构**，通过**双层插件系统**实现极致的扩展性：

```
┌─────────────────────────────────────────────────────────┐
│                    业务层                                │
│  (用户自定义Agent、工作流、规则)                            │
└─────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────┐
│              运行时插件层 (Rhai脚本)                       │
│  • 动态工具注册  • 规则引擎  • 脚本化工作流                  │
│  • 热加载逻辑    • 表达式求值                              │
└─────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────┐
│            编译时插件层 (Rust/WASM)                       │
│  • LLM插件  • 工具插件  • 存储插件  • 协议插件               │
│  • 高性能模块  • 原生系统集成                               │
└─────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────┐
│                  微内核 (mofa-kernel)                    │
│  • 生命周期管理  • 元数据系统  • 通信总线                    │
│  • 任务调度       • 内存管理                               │
└─────────────────────────────────────────────────────────┘
```

#### 双层插件系统的优势

**编译时插件 (Rust/WASM)**

- 极致性能，零运行时开销
- 类型安全，编译期错误检查
- 支持复杂系统调用和原生集成
- WASM沙箱提供安全隔离

**运行时插件 (Rhai脚本)**

- 无需重编译，即时生效
- 业务逻辑热更新
- 用户自定义扩展
- 安全沙箱执行，可配置资源限制

**组合威力**

- 性能关键路径使用Rust插件（如LLM推理、数据处理）
- 业务逻辑使用Rhai脚本（如规则引擎、工作流编排）
- 两者无缝互操作，覆盖99%的扩展场景


## 核心特性

### 1. 微内核架构
MoFA采用**分层微内核架构**，以 `mofa-kernel` 为核心，所有其他功能（包括插件系统、LLM能力、多智能体协作等）均以模块化组件形式构建在微内核之上。

#### 核心设计理念
- **核心简洁性**: 微内核仅包含智能体生命周期管理、元数据系统和动态管理等最基础功能
- **高扩展性**: 所有高级功能通过模块化组件和插件形式扩展，保持内核稳定
- **松耦合**: 组件之间通过标准化接口通信，易于替换和升级

#### 与插件系统的融合
- 插件系统基于微内核的 `Plugin` 接口开发，所有插件（包括LLM插件、工具插件等）均通过 `AgentPlugin` 标准接口集成
- 微内核提供插件注册中心和生命周期管理，支持插件的热加载和版本控制
- LLM能力通过 `LLMPlugin` 实现，将LLM提供者封装为符合微内核规范的插件

#### 与LLM的融合
- LLM作为微内核的插件组件存在，通过 `LLMCapability` 接口提供统一的LLM访问能力
- 所有智能体协作模式（链式、并行、辩论等）均构建在微内核的工作流引擎之上，并通过标准化的LLM插件接口与LLM交互
- 秘书模式同样基于微内核的A2A通信协议和任务调度系统实现

### 2. 双层插件
- **编译时插件**: 极致性能，原生集成
- **运行时插件**: 动态加载，即时生效
- 两者无缝协作，覆盖所有场景

### 3. 智能体协调
- **优先级调度**: 基于优先级的任务调度系统
- **通信总线**: 内置的智能体间通信总线
- **工作流引擎**: 可视化工作流构建器和执行器

### 4. LLM和AI能力
- **LLM抽象层**: 统一的LLM集成接口
- **OpenAI支持**: 内置的OpenAI API集成
- **ReAct模式**: 基于推理和行动的智能体框架
- **多智能体协作**: LLM驱动的智能体协调，支持多种协作模式：
  - **请求-响应模式**: 一对一确定性任务，同步等待返回结果
  - **发布-订阅模式**: 一对多广播任务，多个接收者异步处理
  - **共识机制模式**: 多轮协商与投票决策，适合需要达成一致的场景
  - **辩论模式**: 多个Agent交替发言，通过迭代优化结果质量
  - **并行模式**: 多Agent同时执行并自动聚合结果
  - **顺序模式**: 流水线执行，前一个Agent的输出作为后一个的输入
  - **自定义模式**: 用户自定义协作模式，由LLM解释执行
- **秘书模式**: 提供端到端的任务闭环管理，包括5个核心阶段：接收想法→记录Todo、澄清需求→转换为项目文档、调度分配→调用执行Agent、监控反馈→推送关键决策给人类、验收汇报→更新Todo
  </br>**特点**：
    - 🧠 自主任务规划与分解
    - 🔄 智能Agent调度编排
    - 👤 关键节点人类介入
    - 📊 全流程可观测追溯
    - 🔁 闭环反馈持续优化

### 5. 持久化层
- **多种后端**: 支持PostgreSQL、MySQL和SQLite
- **会话管理**: 持久化的智能体会话存储
- **记忆系统**: 状态化智能体记忆管理

### 6. 监控与可观察性
- **仪表盘**: 内置的Web仪表盘，支持实时指标
- **指标系统**: Prometheus兼容的指标系统
- **追踪框架**: 分布式追踪系统

### 7. Rhai 脚本引擎

MoFA 集成了 [Rhai](https://github.com/rhaiscript/rhai) 嵌入式脚本语言，提供**运行时可编程能力**，无需重新编译即可修改业务逻辑。

#### 脚本引擎核心
- **安全沙箱执行**: 可配置的操作数限制、调用栈深度、循环控制
- **脚本编译缓存**: 预编译脚本，提升重复执行性能
- **丰富的内置函数**: 字符串操作、数学函数、JSON处理、时间工具
- **双向JSON转换**: JSON与Rhai Dynamic类型无缝转换

#### 脚本化工作流节点
- **脚本任务节点**: 通过脚本执行业务逻辑
- **脚本条件节点**: 动态分支判断
- **脚本转换节点**: 数据格式转换
- **YAML/JSON工作流加载**: 通过配置文件定义工作流

#### 动态工具系统
- **脚本化工具定义**: 运行时注册工具
- **参数验证**: 类型检查、范围验证、枚举约束
- **自动JSON Schema生成**: 兼容LLM Function Calling
- **热加载**: 从目录动态加载工具

#### 规则引擎
- **优先级规则**: Critical > High > Normal > Low
- **多种匹配模式**: 首次匹配、全部匹配、有序匹配
- **复合动作**: 设置变量、触发事件、跳转规则
- **规则组管理**: 支持默认回退动作

#### 典型应用场景
| 场景 | 说明 |
|------|------|
| **动态业务规则** | 折扣策略、内容审核规则，无需重新部署 |
| **可配置工作流** | 用户自定义数据处理管道 |
| **LLM工具扩展** | 运行时注册新工具供LLM调用 |
| **A/B测试** | 通过脚本控制实验逻辑 |
| **表达式求值** | 动态条件判断、公式计算 |

## 路线图

### 短期目标
- [ ] Dora-rs运行时支持，用于分布式数据流
- [ ] 完整的分布式追踪实现
- [ ] Python绑定生成
- [ ] 更多LLM提供商集成

### 长期目标
- [ ] 可视化工作流设计器UI
- [ ] 云原生部署支持
- [ ] 高级智能体协调算法
- [ ] 智能体平台
- [ ] 跨进程/跨机器分布式Agent协作
- [ ] 多智能体协作标准协议
- [ ] 跨平台移动端支持
- [ ] 向智能体操作系统演进

## 快速开始

### 安装

将MoFA添加到您的Cargo.toml：

```toml
[dependencies]
mofa-sdk = "0.1.0"
```
运行时模式最适合需要构建完整智能体工作流的场景，具体包括：

  ---
1. 多智能体协同工作场景

运行时提供消息总线（SimpleMessageBus/DoraChannel）和智能体注册系统，支持智能体之间的：
- 点对点通信（send_to_agent）
- 广播消息（broadcast）
- 主题订阅发布（publish_to_topic/subscribe_topic）
- 角色管理（get_agents_by_role）

当需要多个智能体协作完成复杂任务（如主从架构、分工协作）时，运行时的通信机制可以显著简化开发。

  ---
2. 事件驱动的智能体应用

运行时内置事件循环（run_with_receiver/run_event_loop）和中断处理系统，自动管理：
- 事件接收与分发
- 智能体状态生命周期
- 超时与中断处理

适合构建需要响应外部事件或定时器的应用（如实时对话系统、事件响应机器人）。

  ---
3. 分布式智能体系统

当启用 dora 特性时，运行时提供Dora 适配器（DoraAgentNode/DoraDataflow），支持：
- 分布式节点部署
- 跨节点智能体通信
- 数据流管理

适合需要大规模部署、低延迟通信的生产级场景。

  ---
4. 结构化智能体构建

运行时提供AgentBuilder 流式 API，简化智能体的：
- 配置管理
- 插件集成
- 能力声明
- 端口配置

适合需要快速构建标准化智能体的场景，尤其是需要统一管理多个智能体配置时。

  ---
5. 生产级应用

运行时提供完善的：
- 健康检查与状态管理
- 日志与监控集成
- 错误处理机制

适合构建需要稳定运行的生产级应用，而不是简单的插件测试或原型开发。

## CLI 生产级冒烟示例

为了从用户视角验证 CLI 是否可用于生产，MoFA 提供了黑盒冒烟示例：

- 路径：`examples/cli_production_smoke`
- 方式：在隔离的 `XDG_*` 目录中调用已构建的 `mofa` 二进制
- 校验：基于退出码与稳定输出关键字（不做脆弱的全量快照）

当前覆盖命令：

- 顶层命令：`info`、`config path`、`config list`
- Agent：`start`、`status`、`list`、`restart`、`stop`
- Session：`list`、`show`、`export`、`delete`
- Plugin：`list`、`info`、`uninstall`
- Tool：`list`、`info`

运行方式：

```bash
# 仓库根目录
cargo build -p mofa-cli

# examples 工作区
cd examples
cargo run -p cli_production_smoke
cargo test -p cli_production_smoke
```

可选：覆盖二进制路径

```bash
export MOFA_BIN=/absolute/path/to/mofa
```

说明：

- 本次 PR 中该冒烟流程为手动验证（尚未接入 CI 必选项）。
- 断言重点是行为与持久化效果，而不是固定输出格式。

## 文档

- [API 文档](https://docs.rs/mofa-sdk)
- [GitHub 仓库](https://github.com/mofa-org/mofa)
- [示例](examples/)

## 贡献

我们欢迎贡献！请查看我们的[贡献指南](CONTRIBUTING.md)了解更多详情。

## 社区

- GitHub Discussions: [https://github.com/mofa-org/mofa/discussions](https://github.com/mofa-org/mofa/discussions)
- Discord: [https://discord.com/invite/hKJZzDMMm9](https://discord.com/invite/hKJZzDMMm9)
- 若 Discord 邀请链接失效，请在 Discussions 发帖，维护者会提供最新邀请链接。

## 星标历史

[![Star History Chart](https://api.star-history.com/svg?repos=mofa-org/mofa&type=Date)](https://www.star-history.com/#mofa-org/mofa&Date)

## 🙏 致谢

MoFA站在巨人的肩膀上：

- [Rust](https://www.rust-lang.org/) - 性能与安全的完美结合
- [UniFFI](https://mozilla.github.io/uniffi-rs/) - Mozilla的多语言绑定魔法
- [Rhai](https://rhai.rs/) - 强大的嵌入式脚本引擎
- [Tokio](https://tokio.rs/) - 异步运行时基石
- [Ractor](https://github.com/slawlor/ractor) - Actor模型并发框架
- [Dora](https://github.com/dora-rs/dora) - 分布式数据流运行时
- [Wasmtime](https://wasmtime.dev/) - WebAssembly运行时

## 支持

源起之道支持｜Supported by Upstream Labs

## 许可证

[Apache License 2.0](./LICENSE)
