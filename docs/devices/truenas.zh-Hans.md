# TrueNAS {#truenas}

ZFS 存储服务器(TrueNAS SCALE,现称 TrueNAS Community
Edition):每个存储池的健康状况及故障的磁盘、校验和重新同步、
存储池和数据集相对于配额的使用率、快照和复制、磁盘温度和 SMART
自检,TrueNAS 自身触发的告警、服务和机器本身。

!!! warning "仅依据测试实例验证"

    这项集成是依据 TrueNAS 25.04.2.6 构建并验证的,为此专门在
    虚拟机中从官方 ISO 安装——一个 RAIDZ1 存储池、数据集、快照
    和复制任务,然后把同一个池中的一块磁盘下线——而不是针对一台
    已经保存了多年家庭照片的 NAS。虚拟机没有磁盘温度和 SMART
    结果,因此这些部分依据的是 TrueNAS 的源码,而不是观测到的
    实际应答。每次调用都是读取操作。如果有任何问题,请告诉
    我们。

## 这项集成针对的故障 {#the-failure-this-is-for}

一个镜像或 RAIDZ vdev 丢失一块磁盘后仍会继续提供数据。共享仍会
保持挂载,备份仍会继续运行,其他每个工具的网页界面都仍是绿色
的——而这个池现在已经没有冗余了。第二块磁盘的故障就会导致该池
彻底损坏。`truenas_pool_healthy` 会在第一块磁盘故障时就降到 0,
**NAS 存储池降级**规则会触发,监控项页面会指明是哪块磁盘。

## 监控项页面 {#the-device-page}

除了通用图表之外,一个 TrueNAS 监控项还会显示三个面板,读取自
探测存储的数据——打开该页面从不会查询 NAS 本身:

* **存储池与磁盘**——每个存储池及其文字状态、出问题时 ZFS 自身
  给出的说明、使用率、其布局(镜像、RAIDZ、缓存、日志)、其读、
  写和校验和错误计数,以及最重要的,任何非 `ONLINE` 状态磁盘的
  名称。正在进行的校验或重新同步会显示其进度。然后是每块磁盘:
  型号、序列号、类型、容量、所属存储池、温度,以及上次 SMART
  自检的结果。
* **数据集与保护**——每个数据集及其相对于配额的使用率、快照数量
  以及上次有快照任务覆盖它的时间;每个存储池的上次校验及其是否
  逾期;每个复制和定期快照任务及其状态、上次运行、处理的最新
  快照,以及失败时 TrueNAS 自己给出的错误说明。
* **NAS 健康状况**——TrueNAS 自身触发的告警,最严重的排在最前,
  然后是已停止的服务,再然后是机器本身:版本、运行时间、负载、
  内存、CPU。

## 监控内容 {#what-it-watches}

所有指标都以 `dumbmonit_truenas_` 为前缀。

| 指标族 | 指标 | 标签 |
|---|---|---|
| 身份 | `version_info`(版本号,不含较旧版本前面加的 `TrueNAS-SCALE-` 前缀) | `version` |
| 机器 | `uptime_seconds`, `load1/5/15`, `memory_total_bytes`, `cpu_count` | |
| 存储池 | `pool_healthy`(ZFS 自身的判定:`DEGRADED` 和 `FAULTED` 都为 0)、`pool_warning`(重新同步中、功能未启用)、`pool_status_info`、`pool_size/allocated/free_bytes`、`pool_used_percent`、`pool_fragmentation_percent` | `pool`,info 序列还带 `status` |
| 存储池监控项 | `pool_device_errors`(自上次 `zpool clear` 以来的累计值)、`pool_devices_unhealthy`(当该池无法打开时两者都不存在) | `pool`, `kind`(`read`、`write`、`checksum`) |
| 校验 | `pool_scan_running`, `pool_scan_percent`(仅运行期间)、`pool_last_scrub_age_seconds`, `pool_last_scrub_errors`, `pool_scrub_threshold_days` | `pool`, `function`(`SCRUB`、`RESILVER`) |
| 数据集 | `dataset_used_bytes`, `dataset_available_bytes`, `dataset_quota_bytes` 和 `dataset_quota_used_percent`(没有配额时不存在)、`dataset_snapshots`、`dataset_snapshot_age_seconds`(覆盖该数据集的定期任务拍摄的最新快照的年龄;没有这样的任务时不存在)、`dataset_locked`(仅限加密数据集) | `dataset`, `pool` |
| 快照 | `snapshots`(总数) | |
| 磁盘 | `disk_temperature_celsius`, `disk_size_bytes`, `disk_smart_failed`(从未测试过的磁盘不存在) | `disk`, `serial` |
| 告警 | `alerts`——每个级别的活跃告警数,包括零 | `level`(`INFO` … `EMERGENCY`) |
| 任务 | `replication_error`, `replication_last_run_age_seconds`, `snapshot_task_error`, `snapshot_task_last_run_age_seconds`(仅限已启用的任务) | `task` |
| 服务 | `service_running`(仅限设置为随 NAS 启动的服务) | `service` |
| 采集 | `up`, `scrape_errors`, `scrape_duration_seconds` | |

一些值得了解的 TrueNAS 行为:

* **上次校验只存在于存储池的扫描记录中。** API 中没有校验历史,
  重新同步会覆盖该扫描记录:重新同步之后,上次校验的日期就会
  变为未知,直到下一次校验完成,此时
  `pool_last_scrub_age_seconds` 会不存在,而不是显示错误值。
* **一个无法打开的存储池**——已导出,或缺失磁盘过多——仍会
  出现,但没有布局和容量数据。其 `pool_healthy` 为 0;监控项和
  容量相关序列会不存在,而不是显示为零。
* **未设置的配额不是零配额。** 对于"无配额",TrueNAS 应答
  `parsed: null`,DumbMonit 不会为该数据集发布任何配额序列。
  当 `quota` 和 `refquota` 都设置时,`quota` 优先。
* **快照计数来自 ZFS 自身的缓存**(`snapshots_count`),总数来自
  TrueNAS 的快速快照计数——从不会列出任何具体快照。该总数会
  大于各数据集计数之和:它还包含了 TrueNAS 内部数据集的快照,
  而这些在数据集列表中被隐藏了。
* **某个数据集最新快照的年龄**是覆盖它(直接覆盖,或通过递归的
  父级覆盖)的定期快照任务的上次运行时间。TrueNAS 无法在不列出
  全部快照的情况下给出最新快照的日期,因此没有任何任务覆盖的
  数据集就没有年龄数据——这本身就是值得了解的信息。
* **温度是从 TrueNAS 的缓存读取的**(`only_cached`),大约每五
  分钟由 TrueNAS 刷新一次。休眠中的磁盘永远不会被唤醒,没有
  读数的磁盘也不会有对应的序列。
* **绝不会启动任何东西。** 不会启动校验、SMART 测试、更新检查
  或复制运行。探测只会读取。
* **各版本间变化的路径是可选的。** `zfs/snapshot` 在 25.10 中
  变成了 `pool/snapshot`,两者都会被尝试;`smart/test/results`
  在 25.10 中被移除,此时会被静默跳过。

适用的内置规则:监控项不可达、NAS 存储池降级、NAS 磁盘错误、
NAS 存储池即将用尽、NAS 校验发现错误、NAS 校验逾期、NAS 磁盘
SMART 自检失败、NAS 磁盘过热、NAS 数据集接近配额、NAS 复制失败、
NAS 快照任务失败、NAS 快照陈旧、TrueNAS 告警已触发、NAS 服务
已停止。通知会指明涉及的存储池、磁盘、数据集、任务或服务。

## 需要在 TrueNAS 中准备的内容 {#what-to-prepare-in-truenas}

以下步骤正是表单旁边提示所展示的内容。首先要知道一点:在
DumbMonit 使用的 REST API 上,**TrueNAS 没有只读密钥**。它的
只读角色只接入了较新的 WebSocket API;一个用户为 Read-Only
Administrator 的密钥,在每一次 REST 调用上都会被拒绝,返回一个
空的 `403`。这一点已在 TrueNAS 25.04.2.6 上验证:一个
Read-Only Administrator 密钥在尝试的全部 55 个 REST 路径上都
得到了 `403`,而同一个密钥在 WebSocket API 上却能正常工作。
该密钥必须属于一个拥有 Local Administrator 权限的用户——因此它
需要一个专属的用户和组,不多不少。

1. 在网页界面中:Credentials → Groups → Add。按如下方式命名该组,赋予它 Local Administrator 权限。

    ```
    dumbmonit
    ```

2. 为什么是 Local Administrator 而不是 Read-Only Administrator:TrueNAS 只在其 WebSocket API 中接入了只读角色。在 DumbMonit 使用的 REST API 上,只读密钥在每一次调用上都会被拒绝。DumbMonit 本身永远只会读取:它从不启动校验、SMART 测试、更新或复制。

3. Credentials → Users → Add。按如下方式命名该账户,把 dumbmonit 设为它的主组,保持 shell 访问、sudo 和 SSH 关闭:这些都不需要。

    ```
    dumbmonit
    ```

4. 创建密钥:Credentials → Users → API Keys → Add,选择 dumbmonit 用户,给密钥起一个名称。TrueNAS 只会显示一次该密钥:请复制它。在 TrueNAS 24.10 上,该路径是右上角的用户菜单 → API Keys → Add,此时密钥不属于任何用户。

5. 在 DumbMonit 中,输入 NAS 地址,例如 "nas.lan",并粘贴该密钥。请保持 HTTPS:如果 TrueNAS 曾经收到某个密钥通过纯 HTTP 从另一台机器发出,它会撤销该密钥。

!!! warning "警告"

    该密钥能更改 NAS 上的任何内容:TrueNAS 的 REST API 不提供
    只读密钥。请像对待一个能清空你的存储池的账户密码一样对待
    它,只把它留给 DumbMonit,如果泄露就在同一页面撤销它。
    TrueNAS 也自带自签名证书:如果因此连接被拒绝,请在选项中
    勾选"接受无法验证的证书"。

地址:`192.168.1.20`、`nas.lan`、`nas.lan:8443`、`[fd00::20]`,
或完整 URL。如果缺失,会自动补上 `https` 协议和 443 端口。

TrueNAS 已经宣布将在 TrueNAS 26 中移除 REST API,转而使用
WebSocket API。在那之前,REST 是直到 25.10 的每个发行版都提供
的接口。

## 选项 {#options}

| 键 | 标签 | 默认值 | 说明 |
|---|---|---|---|
| `scheme` | 协议 | `https` | 请保持 HTTPS:如果 TrueNAS 收到某个 API 密钥通过纯 HTTP 从另一台机器发出,它会撤销该密钥。 |
| `port` | 网页界面端口 | `443` | 当地址中未给出端口时使用。 |
| `insecure_tls` | 接受无法验证的证书 | `false` | TrueNAS 默认自带自签名证书。 |
| `request_timeout_seconds` | 每次请求的超时时间(秒) | `20` | 每次 API 调用允许的时间,1 到 120。 |
| `datasets` | 监控数据集 | `true` | 相对于配额的使用率、快照数、上次快照变化。 |
| `disks` | 监控磁盘 | `true` | 清单和缓存的温度;休眠中的磁盘永远不会被唤醒。 |
| `smart` | 读取 SMART 测试结果 | `true` | 每块磁盘的上次自检;绝不会启动任何自检。 |
| `alerts` | 读取 TrueNAS 自身的告警 | `true` | TrueNAS 自己保留的告警列表,不包含已忽略的告警。 |
| `tasks` | 监控复制和快照任务 | `true` | 每个任务的状态、上次运行和最新快照。 |
| `services` | 监控服务 | `true` | 设置为随 NAS 启动的服务,以及它们是否在运行。 |

## 内置告警规则

这些规则随 DumbMonit 提供，从一开始就已启用。每条规则都可以在“告警”中修改或关闭；[内置规则参考](../alerting/rules.md#truenas)列出了所有集成的全部规则。

| 规则 | 什么 | 默认阈值 | 保持 | 严重级别 | 提醒 |
|---|---|---|---|---|---|
| NAS 池降级 | ZFS 不再称池为健康(`dumbmonit_truenas_pool_healthy`)。降级池仍然服务其数据,这正是为什么没有人注意直到下一个磁盘失败。监控项页面命名磁盘。 | < 1 | 5 分钟 | 警告 (`critical`) | 6 小时 |
| NAS 磁盘错误 | 池的磁盘已报告读、写或校验和错误因最后的`zpool clear`(`dumbmonit_truenas_pool_device_errors`,每个类型一个系列)。 | > 0 | 15 分钟 | 通知 (`warning`) | 24 小时 |
| NAS 池即将满 | 池超过 85% 满(`dumbmonit_truenas_pool_used_percent`)。ZFS 在满前减慢。在 80% 下清除。 | > 85 % | 30 分钟 | 通知 (`warning`),7 天后升级 | 24 小时 |
| NAS 清理发现错误 | 池的最后清理发现了损坏数据(`dumbmonit_truenas_pool_last_scrub_errors`)。 | > 0 | 5 分钟 | 警告 (`critical`) | 24 小时 |
| NAS 清理逾期 | 池已 45 天未完成清理(`dumbmonit_truenas_pool_last_scrub_age_seconds`)。恢复后缺席,它删除最后清理的记录。 | > 45 天 | 1 小时 | 信息 (`info`) | 7 天 |
| NAS 磁盘 SMART 测试失败 | SMART 自我测试在磁盘日志中失败(`dumbmonit_truenas_disk_smart_failed`)。从未测试的磁盘没有系列。 | > 0 | 5 分钟 | 警告 (`critical`) | 24 小时 |
| NAS 磁盘太热 | 磁盘在 55°C 以上(`dumbmonit_truenas_disk_temperature_celsius`,从 TrueNAS 缓存读取)。在 50°C 下清除。 | > 55°C | 15 分钟 | 通知 (`warning`) | 6 小时 |
| NAS 数据集接近其配额 | 数据集已使用超过 90% 其配额(`dumbmonit_truenas_dataset_quota_used_percent`):在 100% 其写入失败而池仍有空间。无配额,无系列。 | > 90 % | 30 分钟 | 通知 (`warning`) | 24 小时 |
| NAS 复制失败 | 启用的复制任务处于错误(`dumbmonit_truenas_replication_error`):另一侧的副本在变旧。 | > 0 | 10 分钟 | 通知 (`warning`) | 12 小时 |
| NAS 快照任务失败 | 启用的定期快照任务处于错误(`dumbmonit_truenas_snapshot_task_error`)。 | > 0 | 10 分钟 | 通知 (`warning`) | 12 小时 |
| NAS 快照陈旧 | 启用的定期快照任务已 8 天未运行(`dumbmonit_truenas_snapshot_task_last_run_age_seconds`):停止运行的任务不失败,它变安静。 | > 8 天 | 1 小时 | 通知 (`warning`) | 24 小时 |
| TrueNAS 告警提升 | TrueNAS 本身已提升错误或以上级别的告警(`dumbmonit_truenas_alerts`)。其自己的句子在监控项页面。 | > 0 | 10 分钟 | 通知 (`warning`) | 12 小时 |
| NAS 服务停止 | 设置为与 NAS 一起启动的服务已停止(`dumbmonit_truenas_service_running`)。故意关闭的服务没有系列。 | < 1 | 10 分钟 | 通知 (`warning`) | 6 小时 |
| NAS 证书即将到期 | 为 TrueNAS Web 界面提供服务的证书将在 14 天内到期：浏览器、应用和复制对端都会拒绝它(`dumbmonit_truenas_certificate_expiry_days{in_use="1"}`)。 | < 14 天 | 1 小时 | 通知 (`warning`) | 24 小时 |
| NAS 证书已过期 | 为 TrueNAS Web 界面提供服务的证书将在 2 天内到期，或已经过期(`dumbmonit_truenas_certificate_expiry_days{in_use="1"}`)。 | < 2 天 | 15 分钟 | 警告 (`critical`) | 12 小时 |
| NAS 启动池降级 | 启动池不再健康：NAS 仍能从剩下的设备启动，直到那块设备也发生故障(`dumbmonit_truenas_boot_pool_healthy`)。 | < 1 | 10 分钟 | 通知 (`warning`) | 12 小时 |
| NAS 启动池几乎已满 | 启动池使用率超过 85 %：旧的启动环境随每次更新不断累积，并会阻止下一次更新。低于 80 % 时解除(`dumbmonit_truenas_boot_pool_used_percent`)。 | > 85 % | 1 小时 | 通知 (`warning`) | 24 小时 |
| NAS 异地备份任务失败 | 某个云同步、rsync 或云备份任务的最近一次运行失败：远端副本越来越旧(`dumbmonit_truenas_backup_task_failed`)。 | > 0 | 10 分钟 | 通知 (`warning`) | 12 小时 |
| NAS 应用已崩溃 | 某个应用处于崩溃（crashed）状态(`dumbmonit_truenas_app_crashed`)。 | > 0 | 10 分钟 | 通知 (`warning`) | 6 小时 |
| NAS 虚拟机已停止 | 某台设置为随 NAS 启动的虚拟机没有在运行(`dumbmonit_truenas_vm_running`)。 | < 1 | 10 分钟 | 通知 (`warning`) | 6 小时 |

## 已知限制

- 在 DumbMonit 使用的 REST API 上，TrueNAS 没有只读密钥：密钥属于一个完整的管理员。DumbMonit 只做读取，从不启动 scrub、SMART 测试或复制。
- 数据集最新快照的存在时间，取自覆盖它的快照任务的最近一次运行。没有任何任务覆盖的数据集没有这个时间。
- 温度来自 TrueNAS 的缓存，大约每五分钟刷新一次。休眠的磁盘绝不会被唤醒。
- SMART 测试结果在 TrueNAS 25.10 中已从 API 移除，从该版本起会被跳过。
- 证书规则只监视为 Web 界面提供服务的证书。

## 常见错误 {#common-errors}

| 现象 | 可能原因 |
|---|---|
| 提到 Local Administrator 的认证错误 | 该密钥所属的用户不是完整管理员——通常是 Read-Only Administrator。TrueNAS 会在每一次 REST 调用上拒绝这样的密钥,返回一个空的 `403`。 |
| "Invalid API key" 认证错误 | 该密钥被输错、已被删除、已过期——或者因为曾经通过纯 HTTP 从另一台机器发出而被 TrueNAS 撤销。请创建一个新的,并保持 HTTPS。 |
| 证书错误 | 自签名证书:安装一个受信任的证书(内置了 ACME),或启用 `insecure_tls`。 |
| 没有磁盘温度 | NAS 运行在虚拟机中,或 TrueNAS 自启动以来还没有轮询过磁盘。读数来自其缓存。 |
| 没有 SMART 序列 | 该磁盘从未运行过自检,或该 NAS 运行的是 TrueNAS 25.10,它移除了该调用。请在 Data Protection 中安排 SMART 测试。 |
| 没有 `pool_last_scrub_age_seconds` | 该存储池上次的扫描是一次重新同步,它会清除上次校验的记录,或该池从未被校验过。 |
| 某个数据集没有配额序列 | 它没有配额。未设置的配额不是零配额。 |
| "NAS 存储池降级" 但页面没有显示磁盘名称 | 该存储池完全无法打开(已导出,或缺失磁盘过多):TrueNAS 此时不会报告任何布局。 |
| `scrape_errors` 大于零 | 某个可选调用因路径缺失之外的原因失败——通常是超时。其他指标不受影响。 |
