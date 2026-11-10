# Unraid {#unraid}

通过其 GraphQL API 监控 Unraid 服务器:未启动的阵列、被禁用、
缺失或报告读取错误的磁盘、发现错误或数周未运行的校验检查、空间
不足的缓存池、应该运行却未运行的容器,以及虚拟机的状态。

DumbMonit 在 `x-api-key` 请求头中发送一个拥有 Viewer 角色的 API
密钥,调用 `/graphql`。Viewer 角色能读取一切,不能更改任何内容。
该 API 从 7.2 起内置于 Unraid;在 6.12 到 7.1 上则随 Unraid
Connect 插件提供。

DumbMonit 每分钟发送一个必需的查询(版本、阵列、校验以及数据和
缓存磁盘)和四个可选查询,各自独立进行,因此较旧 API 版本缺少
的某个字段,或已关闭的虚拟机,只会让那部分内容缺失:校验检查、
Docker 容器、虚拟机和未读通知。

!!! warning "仅依据厂商文档验证——尚未在真实系统上测试"

    这项集成是根据 Unraid 的 API 文档和已发布的 Unraid API
    GraphQL 模式(字段名称、枚举值、以千字节为单位的大小)构建
    的,而不是针对一台正在运行的服务器。每个查询都是读取操作。
    如果有任何问题,请告诉我们。

## 监控内容 {#what-it-watches}

所有指标都以 `dumbmonit_unraid_` 为前缀。

| 指标 | 标签 | 含义 |
|---|---|---|
| `version_info` | `version` | 固定值 1:Unraid 版本。 |
| `array_started` | | 阵列已启动时为 1。 |
| `array_state_info` | `state` | 固定值 1:`started`、`stopped`、`new_array`、`recon_disk`、`disable_disk`…… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | 阵列容量。 |
| `disk_ok` | `disk`, `role`(`parity`、`data`、`cache`) | 该磁盘状态为 `DISK_OK` 时为 1。空插槽不会出现。 |
| `disk_status_info` | `disk`, `role`, `status` | 固定值 1:Unraid 的状态(`disk_ok`、`disk_dsbl`、`disk_np_missing`、`disk_invalid`……)。 |
| `disk_errors` | `disk`, `role` | 阵列启动以来的读取错误数(Errors 列)。 |
| `disk_temperature_celsius` | `disk`, `role` | 仅在磁盘转动时存在:Unraid 不会为休眠的磁盘报告温度。 |
| `disk_spinning` | `disk`, `role` | 磁盘转动时为 1。 |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | 对于带有文件系统的磁盘:大小、文件系统大小和使用率。 |
| `disks`, `disks_problem`, `disks_with_errors` | | 计数。 |
| `parity_check_status_info` | `status` | 固定值 1:`completed`、`running`、`paused`、`cancelled`、`failed`、`never_run`。 |
| `parity_check_running`, `parity_check_progress_percent` | | 正在进行的检查。 |
| `parity_check_ok` | | 检查完成后:没有同步错误时为 1,有错误或失败时为 0。被取消的检查不能证明任何事,也不会设置任何值。 |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | 上一次检查。 |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Docker 容器。 |
| `container_running`, `container_autostart_stopped` | `container` | 按容器;第二项在设置为随阵列启动的容器未运行时为 1。 |
| `vms`, `vms_running` | | 虚拟机。 |
| `vm_state_info`, `vm_crashed` | `vm`(及 `state`) | 按虚拟机。 |
| `notifications_unread` | `importance`:`alert`、`warning`、`info` | 未读通知。 |
| `registration_ok` | | Unraid 未报告许可证问题时为 1，许可证状态为错误（试用过期、密钥与 U 盘不匹配、密钥文件缺失……）时为 0。 |
| `registration_info` | `state`, `type` | 值为 1：Unraid 报告的许可证状态（`trial`、`pro`、`eexpired`、`eguid`……）以及许可证类型。 |
| `trial_expiry_seconds` | | 距试用许可证结束的时间；仅在试用期间。 |
| `os_updates_expiry_seconds` | | 距许可证失去 Unraid OS 更新资格的时间；许可证没有这个日期时不存在。 |
| `uptime_seconds` | | 自服务器启动以来。 |
| `cpu_usage_percent` | | 处理器负载，所有核心合计。 |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | 内存。不可用的部分算作已用：Unraid 可以归还的缓存不计入。 |
| `swap_used_percent` | | 仅当服务器有交换空间时。 |

许可证和系统负载（运行时间、处理器、内存）来自另外两个可选查询。不认识它们的旧版 API 只会缺少这几行：监控项的其余部分不受影响。

适用的[内置规则](../alerting/rules.md#unraid)：

| 规则 | 什么 | 默认阈值 | 保持 | 严重级别 | 提醒 |
|---|---|---|---|---|---|
| Unraid 阵列已停止 | Unraid 阵列未启动:位于其上的共享、容器和虚拟机不可用(`dumbmonit_unraid_array_started`)。 | < 1 | 10 分钟 | 警告 (`critical`) | 6 小时 |
| Unraid 磁盘被禁用或丢失 | Unraid 磁盘被禁用、丢失或无效:阵列降级运行,再有一次故障就会丢失数据(`dumbmonit_unraid_disk_ok`)。 | < 1 | 5 分钟 | 警告 (`critical`) | 6 小时 |
| Unraid 磁盘读取错误 | Unraid 磁盘自阵列启动以来报告了读取错误:下一次可能被禁用(`dumbmonit_unraid_disk_errors`)。 | > 0 | 5 分钟 | 通知 (`warning`) | 1 天 |
| Unraid 磁盘过热 | Unraid 阵列盘或校验盘已连续十五分钟超过 55 °C(`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`)。 | > 55 °C | 15 分钟 | 通知 (`warning`) | 6 小时 |
| Unraid 校验检查发现错误 | 上一次 Unraid 校验检查失败或发现同步错误:校验不再与数据一致(`dumbmonit_unraid_parity_check_ok`)。 | < 1 | 5 分钟 | 通知 (`warning`) | 1 天 |
| Unraid 校验检查逾期 | 上一次 Unraid 校验检查已超过 40 天:错过了每月检查(`dumbmonit_unraid_parity_check_age_seconds`)。 | > 40 d | 1 小时 | 通知 (`warning`) | 1 天 |
| Unraid 缓存几乎已满 | Unraid 缓存池已满超过 90 %:写入回落到较慢的阵列,或失败(`dumbmonit_unraid_disk_used_percent{role="cache"}`)。 | > 90 % | 30 分钟 | 通知 (`warning`) | 1 天 |
| Unraid 容器已停止 | 设置为随阵列启动的 Docker 容器未运行(`dumbmonit_unraid_container_autostart_stopped`)。 | > 0 | 10 分钟 | 通知 (`warning`) | 6 小时 |
| Unraid 虚拟机已崩溃 | Unraid 报告某台虚拟机已崩溃(`dumbmonit_unraid_vm_crashed`)。 | > 0 | 5 分钟 | 通知 (`warning`) | 6 小时 |
| Unraid 警报未读 | Unraid 发出了一条最高级别（alert）的通知，却没有人读过：磁盘故障、过热、阵列错误……(`dumbmonit_unraid_notifications_unread{importance="alert"}`)。 | > 0 | 10 分钟 | 通知 (`warning`) | 24 小时 |
| Unraid 阵列几乎已满 | 阵列使用率超过 95 %：向共享写入很快就会失败。低于 93 % 时解除(`dumbmonit_unraid_array_used_percent`)。 | > 95 % | 1 小时 | 通知 (`warning`) | 24 小时 |
| Unraid 内存几乎耗尽 | 超过 95 % 的内存被占用已达十五分钟：Unraid 在内存中运行，内存耗尽时容器或虚拟机会被终止。低于 90 % 时解除(`dumbmonit_unraid_memory_used_percent`)。 | > 95 % | 15 分钟 | 通知 (`warning`) | 6 小时 |
| Unraid 许可证问题 | Unraid 报告许可证问题（试用过期、密钥与 U 盘不匹配、密钥文件缺失……）：下次停止之后，阵列可能拒绝启动(`dumbmonit_unraid_registration_ok`)。 | < 1 | 30 分钟 | 通知 (`warning`) | 24 小时 |
| Unraid 试用即将结束 | 试用许可证将在三天内结束：之后阵列将无法启动(`dumbmonit_unraid_trial_expiry_seconds`)。 | < 3 天 | 1 小时 | 通知 (`warning`) | 24 小时 |
| Unraid OS 更新资格即将结束 | 许可证将在 30 天内失去获取 Unraid OS 更新的资格。服务器会继续工作，只是不再收到新版本(`dumbmonit_unraid_os_updates_expiry_seconds`)。 | < 30 天 | 1 小时 | 信息 (`info`) | 7 天 |

## 监控项页面 {#the-device-page}

Array 面板用一句话说明哪里出了问题,显示阵列使用率、上一次校验
检查、正在运行的容器和虚拟机,然后列出每一块磁盘——先是校验盘,
再是数据盘,最后是缓存盘——以及其状态、温度和使用率,还有应该
运行的容器。

## 为 DumbMonit 创建一个只读 API 密钥 {#create-a-read-only-api-key-for-dumbmonit}

1. 在 Unraid 7.2 或更高版本上,该 API 是内置的。在 Unraid 6.12 到 7.1 上,请先从 Apps 标签安装 Unraid Connect 插件:它提供同样的 API,本地使用无需登录 Unraid Connect。

2. 创建一个拥有 Viewer 角色的 API 密钥,该角色能读取一切,不能更改任何内容:在 Settings > Management Access > API Keys 下,或从 Unraid 终端执行。

    ```
    unraid-api apikey --create --name dumbmonit --roles VIEWER --description "DumbMonit monitoring" --json
    ```

3. 复制该密钥。在 DumbMonit 中,输入 Unraid 服务器地址,例如 "tower.lan",并把该密钥粘贴为 API 密钥。如果网页界面通过 HTTPS 提供服务,请选择 https 作为协议。

!!! warning "警告"
    Unraid 不会为已停转的磁盘报告温度,DumbMonit 也绝不会唤醒
    它:一块休眠中的磁盘在转动起来之前根本没有温度数据。

## 凭据 {#credentials}

| 凭据 | 字段 |
|---|---|
| API 密钥 | 拥有 Viewer 角色的密钥,以 `x-api-key` 发送。 |

地址:Unraid 服务器的主机名或 IP、`host:port`,或 URL。选项:

- **协议**(`http`)和**端口**(80):网页界面所使用的。如果在
  Settings > Management Access 下将 SSL 设为 Strict,请使用
  `https`,如果证书是 `myunraid.net` 的,则使用该地址。
- **接受无法验证的证书**:用于自签名证书。
- **每次请求的超时时间**(15 秒)。

## 已知限制

- 不读取 UPS：请改用 [NUT](nut.md) 接入。
- 没有 SMART 详情：DumbMonit 读取 Unraid 为每块磁盘给出的状态、错误和温度，不读取 SMART 属性。
- 不读取容器是否有待安装的镜像更新。
- 许可证、处理器和内存的数值需要较新版本的 Unraid API。在没有这些数据的版本上，这几行及其规则只是不存在。

## 故障排查 {#troubleshooting}

"Unraid refused the API key (401)":该密钥错误或已被删除。

"Unraid accepted the API key but refused to answer (403)":该密钥
没有角色,或角色权限低于 Viewer。

没有容器、虚拟机或校验数据:说明该可选查询失败了(虚拟机已关闭,
或较旧的 API 版本没有该字段);其余内容仍会被正常读取。原因会
记录在服务器日志的 debug 级别中。
