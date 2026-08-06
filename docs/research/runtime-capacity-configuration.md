# Runtime 与 Capacity 配置组织调研

> 最终实施状态（2026-08-06）：调研中的“按所有者划分”结论被保留，但稳定的容量、超时、窗口、TTL 和任务周期最终全部固化为拥有模块中的带注释常量；触顶拒绝或丢弃时记录 error。YAML 只保留部署差异项和受控日志覆盖。下文映射表是决策前的调研过程，不代表当前 schema。

## 结论先行

本次调研没有发现成熟项目把“一切运行时参数”统一放进一个通用 `runtime`，也没有发现它们把所有上限统一归入一个通用 `capacity`。相反，参数通常跟随拥有该行为和生命周期的子系统，例如 `server`、`socket`、`match`、`database`、`storage`、`rpc`、`telemetry`、`shutdown`、`overload`。两个明显使用 `runtime` 一词的项目，含义也都很窄：Nakama 的 `runtime` 是脚本 VM，Envoy 的 `runtime` 是可重载覆盖层，而不是“进程运行期间会用到的所有参数”。[Nakama 配置参考](https://heroiclabs.com/docs/nakama/getting-started/configuration/#runtime)，[Envoy Runtime 说明](https://www.envoyproxy.io/docs/envoy/latest/configuration/operations/runtime.html)

因此，XKK 当前设计的主要问题不是“只抽出了四个 Common Runtime 字段，所以抽得还不够”，而是 `runtime` 和 `capacity` 本身都混合了多个所有者。继续从角色 `runtime` 中寻找更多同名字段搬入一个更大的 Common Runtime，只会让边界更模糊。

推荐下一步先按拥有子系统重组，再决定值的作用域：

1. 去掉宽泛的顶层 `runtime` 和 `capacity` 语义；保留真正有领域含义的 `LogicRuntime`，但其配置应归在 `players` 或 `player_runtime` 下。
2. 队列、缓存、连接、并发和业务承载量跟随各自子系统，不集中到一个 `capacity` 桶。
3. “同名、同值”不是共享配置所有权的充分条件。只有行为语义、变更原因、验证规则和生效生命周期都相同，才适合在 `common.yaml` 中只声明一次。
4. Rust 类型可以共享，而 YAML 值仍由角色拥有。例如所有角色可复用同一个 `RpcLimits` 类型，但 `rpc.pending_capacity` 仍在各角色文件中显式配置。
5. 本轮保持启动时静态加载和 fail-fast。将来若需要动态配置，应像 NATS、TiKV、Envoy 一样给字段建立明确的可重载清单、作用域、校验和原子应用机制，而不是自动重新读取整份 YAML。[NATS 配置管理](https://docs.nats.io/learn/deployment/config-management)，[TiKV 动态配置](https://docs.pingcap.com/tidb/stable/dynamic-config/)，[Envoy Runtime 原子性](https://www.envoyproxy.io/docs/envoy/latest/configuration/operations/runtime.html#atomicity)

## 调研范围与方法

调研对象覆盖游戏后端、游戏服务器编排、分布式存储、消息服务器、数据库和代理：Nakama、Agones/Kubernetes、etcd、TiKV、NATS、CockroachDB、Envoy。只使用项目官方文档、官方 API 参考和官方仓库链接；没有引用博客汇总或第三方教程。

重点核对六个问题：

- 全局默认、集群、角色、节点和实例如何分层；
- 行为参数与容量/资源边界是否分开；
- 哪些参数启动时固定，哪些支持动态更新；
- 哪些使用代码默认值，哪些要求生产显式配置；
- 是否支持 include、override 或 merge，以及其边界；
- 参数是否按拥有子系统组织，而非笼统归入 `runtime`/`capacity`。

## 证据矩阵

| 项目 | 配置作用域 | Runtime 的真实含义 | Capacity/limit 的组织 | 更新模型 | 对 XKK 最有价值的模式 |
| --- | --- | --- | --- | --- | --- |
| Nakama | 单节点启动 YAML，CLI 覆盖文件，文件覆盖代码默认 | 专指 Lua/JavaScript 等脚本运行引擎 | 分散在 `database`、`socket`、`match`、`leaderboard`、`matchmaker` 等拥有者中 | 官方配置入口是启动文件和 CLI | 不要把队列、超时、业务限制都塞入 runtime；按游戏子系统归档 |
| Agones + Kubernetes | Fleet 模板、GameServer 实例、Pod/Container 资源 | 没有通用 runtime 配置桶 | Fleet 副本、GameServer 玩家/房间容量、Pod CPU/内存各自建模 | Kubernetes API 持续协调；Fleet 变更滚动更新；部分游戏容量运行时可调 | 区分“实例数量”“业务席位”“进程物理资源”三种容量 |
| etcd | 每成员启动配置；集群成员关系另有 API | 没有通用 runtime 配置桶 | YAML 是扁平 flag map，但官方按 Member、Clustering、Security、Auth、Logging 等责任分组 | 配置文件启动时选定；文件存在时完全排除 flags/env | 明确唯一配置来源；扁平配置虽简单，但类型和单位容易产生陷阱 |
| TiKV/TiDB | 组件、全部实例、单实例、PD 集群配置 | 没有通用 runtime 配置桶 | `server`、`raftstore`、`storage.block-cache`、`storage.flow-control`、`quota`、各 DB/CF 分层 | 受支持字段可动态修改，并明确持久化位置和作用域 | 按子系统拥有容量；自适应默认与必须显式的安全预算分开 |
| NATS | 一个主配置，可 relative include；server/account/JetStream 多层 | 没有通用 runtime 配置桶 | 连接、payload、pending、account、JetStream 存储分别限制 | 每个字段公开 Reloadable 属性；验证后原子 reload | 共享片段用 include，不做无约束 merge；字段级声明静态/动态 |
| CockroachDB | 节点启动 flags、动态 cluster settings、Kubernetes 资源 | Go runtime 只出现在专门的 GC 内存软上限 | OS/Pod 资源、节点内存子预算、磁盘 store、SQL 子系统容量分开 | 节点 flags 重启生效，cluster settings 在线传播 | 区分部署资源信封与应用内部预算，并校验总预算关系 |
| Envoy | Bootstrap、static/dynamic xDS、layered runtime | 专指可重载键值覆盖层 | 过载保护在 `overload_manager`，连接限制在 listener/global monitor，集群请求限制在 circuit breaker | 后层覆盖前层；文件、RTDS、admin 可更新；bootstrap 仍独立 | 只有真正动态的参数才叫 runtime；覆盖层必须显式排序和原子化 |

## 项目证据

### 1. Nakama：`runtime` 是脚本引擎，不是杂项区

Nakama 用一个 YAML 配置节点，配置文件覆盖内置默认值，CLI flag 再覆盖配置文件。官方说明所有选项都有默认值，生产只需覆盖子集，但同时明确要求生产必须替换 `socket.server_key`、`session.encryption_key` 和 `runtime.http_key` 等安全值。[配置优先级与生产必改项](https://heroiclabs.com/docs/nakama/getting-started/configuration/#server-configuration)

它按拥有行为的子系统组织参数：

- `database.max_open_conns` 和 `database.max_idle_conns` 属于数据库连接池；[Database 配置](https://heroiclabs.com/docs/nakama/getting-started/configuration/#database)
- `match.call_queue_size`、`match.input_queue_size`、`match.join_attempt_queue_size` 属于权威对局；[Match 配置](https://heroiclabs.com/docs/nakama/getting-started/configuration/#match)
- `socket.max_message_size_bytes`、`socket.outgoing_queue_size` 和各种读写/心跳超时属于客户端 socket；[Socket 配置](https://heroiclabs.com/docs/nakama/getting-started/configuration/#socket)
- `leaderboard.callback_queue_size` 和 worker 数量归排行榜；[Leaderboard 配置](https://heroiclabs.com/docs/nakama/getting-started/configuration/#leaderboard)
- `matchmaker.max_tickets` 和匹配周期归 matchmaker；[Matchmaker 配置](https://heroiclabs.com/docs/nakama/getting-started/configuration/#matchmaker)
- `runtime.event_queue_size`、JS/Lua VM 最小和最大实例数、脚本路径才归 `runtime`。[Runtime 配置](https://heroiclabs.com/docs/nakama/getting-started/configuration/#runtime)

这说明即便在真正的游戏后端里，容量也不是一个总桶。队列容量跟随生产和消费它的模块；`runtime` 只在存在明确的脚本运行时领域对象时成立。

### 2. Agones/Kubernetes：三种容量分三层表达

Agones 的 Fleet 用 `replicas` 表达要维持多少个 Ready/Allocated GameServer，并把完整 GameServer 规格作为模板；修改模板时由 RollingUpdate 或 Recreate 策略处理。[Fleet 规格](https://agones.dev/site/docs/reference/fleet/)

单个 GameServer 的业务承载量不放在 Pod 资源里。玩家、房间等通过具名 `counters`/`lists` 声明，每个键有自己的 `capacity`；容量和值位于具体 GameServer 实例状态，能够通过 SDK 或 Kubernetes API 在运行期调整，但键必须在 GameServer 创建时声明。[GameServer 规格](https://agones.dev/site/docs/reference/gameserver/)，[Counters and Lists](https://agones.dev/site/docs/guides/counters-and-lists/)

CPU、内存和临时存储则继续使用 Kubernetes 容器 `resources.requests`/`resources.limits`。request 用于调度和保留，limit 由 kubelet/container runtime 最终交给内核实施；它们不是游戏业务容量。[Kubernetes Pod/Container 资源管理](https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/)

可归纳为三个不同维度：

- Fleet `replicas`：水平实例容量；
- GameServer `players`/`rooms` capacity：业务占用容量；
- Pod CPU/memory resources：进程物理资源信封。

把三者统一命名为 `capacity` 会丢失调度者、执行者和变更生命周期。

### 3. etcd：单一来源清晰，但扁平 map 暴露了类型代价

etcd 可由 flags、环境变量或 YAML 配置。flags 优先于环境变量；一旦指定配置文件，其他 flags 和环境变量会被忽略。这个规则避免了多来源隐式合并。[etcd 配置来源与优先级](https://etcd.io/docs/v3.6/op-guide/configuration/#configuration-options)

配置文件本身只是 flag 名到值的扁平 YAML map，不过官方文档仍按 Member、Clustering、Security、Auth、Profiling、Logging 等职责分组。`snapshot-count`、`max-snapshots`、`quota-backend-bytes`、`max-txn-ops`、`max-request-bytes` 和 gRPC keepalive 全部位于 Member 责任域，而不是一个抽象 `capacity`。[etcd Member 配置](https://etcd.io/docs/v3.6/op-guide/configuration/#member)

扁平模型也展示了风险：部分 duration 在 CLI 接受 `10m`/`5s`，但在配置文件中因 Go 反序列化限制只能使用纳秒整数。官方专门记录了这一差异。[etcd Configuration file](https://etcd.io/docs/v3.6/op-guide/configuration/#configuration-file)

对 XKK 的启示不是复制 etcd 的扁平格式，而是保留其“显式选择一个启动文件、配置来源不混搭”的原则，同时继续使用强类型 duration 和嵌套结构避免单位错误。

### 4. TiKV/TiDB：按子系统层级组织，并对动态能力逐项登记

TiKV 使用分层 TOML。内存和容量并未放进一个统一区：总实例内存是 `memory-usage-limit`，共享 block cache 是 `storage.block-cache.capacity`，I/O 限流是 `storage.io-rate-limit.max-bytes-per-sec`，写入保护阈值在 `storage.flow-control`，RPC 内存和消息/快照并发在 `server`，线程数则分布在 `server`、`raftstore`、`readpool`、RocksDB 等实际执行子系统。[TiKV 配置文件](https://docs.pingcap.com/tidb/stable/tikv-configuration-file/)

默认值也不是一刀切。`memory-usage-limit` 通常按可用系统内存计算，`storage.block-cache.capacity` 根据系统内存和存储引擎自适应；gRPC、Scheduler、ReadPool 线程池也可按 CPU 自动选择，并建议只在指标证明需要时调整。[TiKV 内存配置](https://docs.pingcap.com/tidb/stable/tikv-configuration-file/#memory-usage-limit)，[TiKV 线程池调优](https://docs.pingcap.com/tidb/stable/tune-tikv-thread-performance/)

动态修改不是“所有 YAML 都可 reload”。官方维护一张可动态修改字段清单。TiKV 支持按整个组件或单实例修改，成功值会写回配置文件；PD 配置则是全实例共享并持久化在 etcd。批量修改还明确说明不保证跨实例原子性。[TiKV/TiDB 动态配置](https://docs.pingcap.com/tidb/stable/dynamic-config/)

这给 XKK 两点直接启示：

- worker/shard 等性能调优参数若不直接界定 retained work，可以有基于 CPU 的代码默认和显式 override；
- 队列、dirty players、inflight bytes 等安全边界仍应显式，并归拥有 admission 的模块。

### 5. NATS：include 复用文件，reloadability 由字段声明

NATS 使用一个显式主配置文件。`include` 允许把配置拆成相对主文件的片段，启动只指定顶层文件；它解决复用，但没有引入任意对象 deep merge 或不透明的覆盖优先级。[NATS Include Directive](https://docs.nats.io/reference/config/#include-directive)

参数按连接、认证、cluster、gateway、leafnode、JetStream、logging、monitoring 和 limits 等责任组织。`max_connections`、`max_payload`、`max_pending`、`max_subscriptions` 是 server 连接协议限制；JetStream 的 memory/file 上限属于 `jetstream`；账户还能有独立资源限制。[NATS 配置参考](https://docs.nats.io/reference/config/)，[JetStream 资源配置](https://docs.nats.io/running-a-nats-service/configuration/resource_management)

NATS 的配置参考为字段公开 `Reloadable` 属性。例如 bind host/port 和 server identity 不能普通 reload，部分认证、cluster、日志与连接政策可以。官方配置管理流程要求先用 `nats-server -t` 校验，再发 SIGHUP；失败时旧配置继续生效，成功 reload 是原子的。[NATS 字段 Reloadable 表](https://docs.nats.io/reference/config/#properties)，[Validate then reload](https://docs.nats.io/learn/deployment/config-management#validate-then-reload)

这比“runtime 下的字段都能在运行时改”更可靠：动态性是每个字段的元数据，不是由它位于某个 YAML section 推断出来。

### 6. CockroachDB：节点资源、应用子预算、集群动态设置分开

CockroachDB 明确区分两类作用域：节点级设置通过 `cockroach start` flags 提供，修改需要重启节点；cluster settings 作用于整个集群，可通过 SQL 在线修改并传播。[Node flags 与 Cluster settings](https://www.cockroachlabs.com/docs/v26.2/cockroach-start)，[Cluster Settings](https://www.cockroachlabs.com/docs/v26.2/cluster-settings)

节点容量也按用途分开：`--cache` 是 block cache，`--max-sql-memory` 是 SQL 临时内存，`--max-disk-temp-storage` 是溢写空间，`--store` 描述每个存储设备及最大 size。官方还约束 `--cache`、`--max-sql-memory` 和 `--max-tsdb-memory` 的总和不能超过进程可用内存的一定比例，并要求生产显式配置 cache。[cockroach start flags](https://www.cockroachlabs.com/docs/v26.2/cockroach-start#flags)

在 Kubernetes 中，Pod 的 CPU/memory requests/limits 是外层资源信封；CockroachDB 的 cache 和 SQL memory 是信封内的应用子预算；PVC storage 又是独立持久存储容量。官方清楚地把这些字段放在不同 CRD/Helm sections 中。[CockroachDB Kubernetes Resource Management](https://www.cockroachlabs.com/docs/v26.2/configure-cockroachdb-kubernetes)

XKK 应同样避免把未来可能出现的容器内存、业务队列和玩家数量放在一个 `capacity` 下。物理资源属于部署层，稳定的应用内部预算属于拥有模块的代码硬约定。

### 7. Envoy：只有可重载覆盖层才叫 Runtime

Envoy bootstrap 本身明确区分 `static_resources`、`dynamic_resources`、`layered_runtime`、`overload_manager`、stats 和 admin 等责任。[Envoy Bootstrap API](https://www.envoyproxy.io/docs/envoy/latest/api-v3/config/bootstrap/v3/bootstrap.proto.html)

Envoy `runtime` 是由 static、disk、RTDS、admin 等来源组成的可重载虚拟文件系统；后层覆盖前层，并支持针对 service cluster 的专门层。磁盘更新建议使用不可变目录和原子 symlink swap。[Envoy Runtime Layering](https://www.envoyproxy.io/docs/envoy/latest/configuration/operations/runtime.html#layering)，[Envoy Runtime Atomicity](https://www.envoyproxy.io/docs/envoy/latest/configuration/operations/runtime.html#atomicity)

资源保护没有放进 runtime 大桶。`overload_manager` 监测内存、CPU、连接等资源压力，按阈值触发拒绝请求、停止接收连接、缩堆或缩短超时；全局 downstream connection limit 也属于对应 resource monitor。[Envoy Overload Manager](https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/operations/overload_manager)，[Overload Manager 配置](https://www.envoyproxy.io/docs/envoy/latest/configuration/operations/overload_manager/overload_manager)

Envoy 说明“公共默认 + 服务覆盖”是可以成立的，但前提是它被设计成独立控制平面：层次顺序显式、更新原子、错误层可观测、admin 受保护。XKK 当前没有这种需求和机制，不应仅为了减少 YAML 重复就复制 layered override。

## 跨项目归纳

### `runtime` 不是“启动后会使用的配置”

“运行期间会读取”不能成为配置所有权。按这个定义，几乎所有配置都属于 runtime。成熟项目只在存在明确对象时使用该名称：脚本 VM、语言 runtime 或专门的动态覆盖层。

XKK 的 `shutdown_drain_seconds`、`metrics_interval_seconds`、`rpc_timeout_ms`、`mail_lock_seconds` 和 `player_ttl_seconds` 分别属于 lifecycle、telemetry、RPC caller、mail、player residency；它们只是都以 duration 表达，并不属于同一个领域对象。

### `capacity` 至少包含五类不同东西

| 类别 | 示例 | 真正所有者 | 默认策略 |
| --- | --- | --- | --- |
| 物理资源信封 | CPU、内存、磁盘、Pod limit | 部署平台/进程管理器 | 部署显式；不进入业务 YAML |
| retained-work 硬边界 | mailbox、outbox、pending RPC、dirty players、queue | 保存这些对象的模块 | XKK 使用带注释的代码硬常量 |
| 协议/输入边界 | body bytes、message bytes、subscriptions | listener/protocol parser | 子系统显式，校验上下游兼容 |
| 业务承载/准入 | 在线玩家、登录队列、每玩家邮件、match tickets | 业务 admission/policy | 代码硬约定，拒绝路径可观测 |
| 执行并行度/调优 | workers、shards、batch size | scheduler/executor/persistence | 安全时可用代码自适应默认；必要时 override |

同一个数字即便都是 `1024`，如果一个限制内存保留量、一个限制请求并发、一个只是 batch 调优，它们也不能共享配置所有权。

### 共享类型、共享默认、共享值是三件不同的事

- 共享类型：多个角色都使用 `RpcLimits`、`Lifecycle`、`Telemetry` 结构；这是 `xkk-config` 的代码复用。
- 共享默认：多个角色未显式配置时使用同一个代码默认；只适合不构成 retained-work 合同的安全调优项。
- 共享值：整个部署只有一个值并在 `common.yaml` 声明一次；只有语义和变更原因完全一致时成立。

不能从“字段名字相同”直接推导共享值。例如 XKK 当前 `service_load_interval_seconds`：Auth 用来刷新 Gate 在线数，Logic 用来发布自己的在线数，Gate 同时用于发布 Gate 和刷新 Logic。它们现在恰好都是 3 秒，但生产者 TTL、消费者新鲜度和 Redis 成本的变更原因不同，应拆成 `publish_interval` 与 `refresh_interval`，而不是提到 Common Runtime。

### 静态与动态配置应由字段能力决定

XKK 最终只把 listener、instance identity、DSN、secret 和日志作为静态启动配置；mailbox shards、队列构造容量等稳定约定通过改代码和重新构建生效。

以后若引入动态配置，应先定义：

- 字段是否可重载；
- 作用于 cluster、role、全部实例还是单实例；
- 更新是否持久化，持久化到哪里；
- 是否原子，以及部分失败如何报告；
- 新值如何验证，与现有状态冲突如何处理；
- 谁可以修改，如何审计和回滚。

日志级别、部分 admission/rate policy 可能适合动态化；listener、DSN、shards 和绝大多数容器容量不适合直接热改。

## XKK 当前字段归类建议

以下是配置语义归类，不是本轮要实施的最终 YAML schema。目标是先确定所有者，再讨论哪些 section 放 common。

### 跨服务字段

| 当前字段 | 建议所有者/section | 是否建议 Common | 判断 |
| --- | --- | --- | --- |
| `capacity.rpc_pending` | `rpc.pending_capacity` | 共享 Rust 类型，值留在角色 | 同一 `xframe` 机制，但各角色流量和拒绝预算不同；属于 retained work |
| `capacity.write_queue` | `service_transport.write_queue_capacity` | 共享 Rust 类型，值留在角色 | 属于内部连接 transport，不属于所有业务容量 |
| `runtime.rpc_timeout_ms` | `rpc.call_timeout`，必要时按下游/操作拆分 | 不建议 | timeout 由 caller 的失败语义拥有；Gate、Logic、Public 的调用路径不同 |
| `runtime.shutdown_drain_seconds` | `lifecycle.shutdown_drain_timeout` | 条件式 | 若它是整个部署统一 SLO，可由 common 唯一声明；若 Logic flush 与无状态 Query 的预算不同，应角色化。当前同为 10 秒不足以证明同一所有权 |
| `runtime.metrics_interval_seconds` | `telemetry.metrics.report_interval` | 最有可能 Common | 当前都用于周期指标任务，若确认输出和调度语义一致，可在 common 只声明一次 |
| `runtime.service_load_interval_seconds` | `service_load.publish_interval` / `service_load.refresh_interval` | 不建议 | 当前同名字段承担发布、刷新或两者，语义已经不同 |
| `log` | `log` common baseline + typed role override | 保持现状 | NATS/Envoy 等也对日志建立专门配置与受控覆盖；它不是 general merge 的依据 |

一个重要修正是：公共化不是把 `Runtime` 结构拆成 `CommonRuntime + RoleRuntime`，而是让真正公共的 concern 自己成为 `telemetry`、`lifecycle` 等 section。剩下的字段跟随业务模块。

### Auth

| 当前字段 | 建议 section | 说明 |
| --- | --- | --- |
| `capacity.max_http_body_bytes` | `http.limits.max_body_bytes` | HTTP parser/input 边界 |
| `capacity.max_inflight_requests` | `http.admission.max_inflight_requests` | HTTP handler 并发准入 |
| `capacity.login_global_limit` | `login.rate_limit.global` | 必须与窗口放在一起 |
| `capacity.login_per_ip_limit` | `login.rate_limit.per_ip` | 同上 |
| `runtime.login_rate_window_ms` | `login.rate_limit.window` | 不是通用 runtime |
| `capacity.role_admission_limit` | `role_admission.limit` | 业务准入策略 |
| `runtime.role_admission_window_ms` | `role_admission.window` | 与 limit 是一个不可拆合同 |
| `capacity.gate_player_capacity` | `gate_selection.player_capacity_per_instance` | Auth 用它估算 Gate 剩余席位；它不是 Auth 本地容量 |
| `capacity.login_queue_capacity` | `login_queue.capacity` | 登录队列 retained-work/业务上限 |
| `runtime.login_queue_retry_seconds` | `login_queue.retry_after` | 与队列拒绝/等待协议同属一个模块 |
| `runtime.login_queue_entry_ttl_seconds` | `login_queue.entry_ttl` | 与 queue capacity 同时验证 |
| `runtime.account_lock_seconds` | `account_lock.ttl` | 账户并发控制语义 |

### Gate

| 当前字段 | 建议 section | 说明 |
| --- | --- | --- |
| `capacity.max_external_handshakes` | `client_transport.admission.max_handshakes` | 外部连接握手准入 |
| `capacity.max_external_connections` | `client_transport.admission.max_connections` | 外部连接存量上限 |
| `capacity.client_mailbox` | `client_runtime.mailbox_capacity` | Gate 每客户端 mailbox retained work |
| `capacity.outbox_messages` | `session.outbox_capacity` | 重连会话保存的消息量 |
| `runtime.resume_seconds` | `session.resume_ttl` | 与 outbox/reconnect 属于同一会话状态机 |
| `runtime.reconnect_total` | `session.reconnect.total_limit` | 计数与窗口一起组织 |
| `runtime.reconnect_window_seconds` | `session.reconnect.window` | 同上 |
| `runtime.reconnect_window_count` | `session.reconnect.window_limit` | 同上 |
| `capacity.client_request_window_count` | `session.request_rate.long_window.limit` | limit 和 duration 不应分别落在 capacity/runtime |
| `runtime.client_request_window_ms` | `session.request_rate.long_window.duration` | 同上 |
| `capacity.client_burst_count` | `session.request_rate.burst.limit` | 同上 |
| `runtime.client_burst_window_ms` | `session.request_rate.burst.duration` | 同上 |
| `capacity.shutdown_cleanup_concurrency` | `lifecycle.shutdown_cleanup_concurrency` | 生命周期执行并行度，不是业务 capacity |
| `runtime.service_load_interval_seconds` | `service_load.publish_interval` + `service_load.logic_refresh_interval` | Gate 同一个值驱动两种任务，应先拆语义 |

### Logic

Logic 是唯一已经存在明确 `LogicRuntime` 领域对象的角色，因此可以保留 `player_runtime` 这个名字，但不要与进程级 operational runtime 混在一起。

| 当前字段 | 建议 section | 说明 |
| --- | --- | --- |
| `capacity.resident_players` | `player_runtime.resident_capacity` | 常驻玩家硬上限 |
| `runtime.player_ttl_seconds` | `player_runtime.resident_ttl` | 与 resident eviction 同属一个对象 |
| `capacity.mailbox_shards` | `player_runtime.mailbox_shards` | 内部调度结构；是否显式可另行评估，但改变需重启 |
| `capacity.max_inflight_calls` | `player_runtime.admission.global.max_calls` | 全局 retained calls |
| `capacity.max_inflight_kib` | `player_runtime.admission.global.max_kib` | 全局 retained bytes |
| `capacity.max_calls_per_gid` | `player_runtime.admission.per_gid.max_calls` | 单玩家公平性/串行邮箱边界 |
| `capacity.max_kib_per_gid` | `player_runtime.admission.per_gid.max_kib` | 同上 |
| `capacity.max_dirty_players` | `player_persistence.max_dirty_players` | 未持久化状态硬边界 |
| `capacity.batch_save_count` | `player_persistence.save_batch_size` | persistence 执行策略，不是同类硬容量 |
| `runtime.rpc_timeout_ms` | `player_rpc.call_timeout` 或按具体调用命名 | caller 语义；不要仅因其他角色也有 3000ms 而 common |
| `runtime.service_load_interval_seconds` | `service_load.publish_interval` | Logic 当前只发布自身在线数 |

### Public

| 当前字段 | 建议 section | 说明 |
| --- | --- | --- |
| `capacity.max_mails_per_player` | `mail.max_per_player` | 明确业务数据规则 |
| `runtime.mail_lock_seconds` | 已删除 | gid 哈希把玩家固定到一个 Public；Public Player Data 由 xlru 驻留并通过聚合内读写锁串行化 |
| `capacity.rpc_pending` | `rpc.pending_capacity` | incoming RPC retained work |
| `capacity.write_queue` | `service_transport.write_queue_capacity` | 内部 transport |
| `runtime.rpc_timeout_ms` | `mail.rpc_call_timeout` 或对应 outbound caller | 由调用语义拥有 |

### Query

| 当前字段 | 建议 section | 说明 |
| --- | --- | --- |
| `capacity.max_http_body_bytes` | `http.limits.max_body_bytes` | HTTP input 边界 |
| `capacity.max_inflight_requests` | `http.admission.max_inflight_requests` | HTTP 并发准入 |
| `capacity.max_gamer_ids` | `gamer_query.max_ids_per_request` | API 业务请求边界 |
| `capacity.rpc_pending` | `rpc.pending_capacity` | xframe RPC retained work，不应与 HTTP 上限混放 |

## 可选结构方案

### 方案 A：按 concern/子系统直接分组（推荐）

示意，不代表最终字段命名：

```yaml
node: ...
listeners: ...

rpc:
  pending_capacity: 100000
  call_timeout: 3s

service_transport:
  write_queue_capacity: 1024

lifecycle:
  shutdown_drain_timeout: 10s

telemetry:
  metrics:
    report_interval: 10s

session:
  outbox_capacity: 256
  resume_ttl: 60s
  reconnect:
    total_limit: 10
    window: 60s
    window_limit: 5
```

优点是字段靠近实现模块和验证关系，新增模块自然新增 section；缺点是迁移字段较多，需要同步调整 typed config 和文档。

### 方案 B：保留 `runtime`/`capacity`，内部再按子系统嵌套

例如 `capacity.rpc.pending`、`capacity.session.outbox`、`runtime.session.resume_ttl`。它比当前扁平结构好，但 limit 与 window 仍被人为拆开，调用方要跨两个树组装一个业务合同。只适合作为低成本过渡，不建议作为长期结构。

### 方案 C：Common baseline + role deep override

Envoy 证明这种模型可以工作，但需要显式层顺序、原子更新、配置 dump、错误层统计和受控 admin。XKK 当前只是启动 YAML 组合，没有必要承担这套复杂度。除现有 typed log override 外，不建议引入 unrestricted deep merge。

## 推荐决策顺序

1. 先为每个字段确定拥有模块、容量类别和启动/动态生命周期。
2. 让 limit 与其 window/TTL/rejection behavior 放进同一 typed section。
3. 提取可复用 Rust config 类型，但暂不据此移动 YAML 值。
4. 只把真正部署级、所有角色语义一致的值放进 `common.yaml`；role 文件不重复该字段。
5. 将 retained-work 容量固化为拥有模块中的带注释常量，并由 ADR 0015 取代 ADR 0005。
6. 编写最终 schema 示例和迁移表，经过确认后再改代码。

## 对本轮 Common Runtime 尝试的判断

把 `shutdown_drain_seconds`、`service_load_interval_seconds`、`metrics_interval_seconds`、`rpc_timeout_ms` 四个字段抽到 Common Runtime 不应继续扩展，原因分别不同：

- `service_load_interval_seconds` 已经确认有三种任务组合，不能 common；
- `rpc_timeout_ms` 属于 caller，不能因默认值一样就 common；
- `shutdown_drain_seconds` 需要先决定它是统一部署 SLO，还是各服务 retained-work 的退出预算；
- `metrics_interval_seconds` 最接近真正公共字段，但它应归 `telemetry.metrics`，而不是 Common Runtime。

所以正确方向不是再找第五、第六个字段放进 Common Runtime，而是取消这个模糊中间层。最终 Common Configuration 不包含 `telemetry` 或 `lifecycle`；稳定周期和生命周期预算由代码硬约定承担。
