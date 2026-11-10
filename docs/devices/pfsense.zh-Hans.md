# pfSense {#pfsense}

通过社区 REST API 插件(`pfSense-pkg-RESTAPI`,版本 2)监控 pfSense
防火墙:宕机、丢包或响应缓慢的网关,无链路的已启用接口,停止运行
的已启用服务,以及 CPU、内存、磁盘和温度。

DumbMonit 在 `X-API-Key` 请求头中发送一个 API 密钥。密钥恰好
拥有其所属 pfSense 用户的权限:下面的 `dumbmonit` 用户只拥有
DumbMonit 读取的五个页面的 `GET` 权限,因此该密钥只能读取这些
页面,不能更改任何内容。DumbMonit 每分钟并行读取
`/api/v2/status/system`、`/api/v2/status/gateways`、
`/api/v2/status/interfaces`、`/api/v2/status/services` 和
`/api/v2/system/version`,以及在授予了相应权限时的
`/api/v2/system/restapi/version`。

!!! warning "仅依据厂商文档验证——尚未在真实系统上测试"

    这项集成是根据该 REST API 插件已发布的文档及其模型源码
    (字段名称、单位、应答信封及其错误标识符)构建的,而不是
    针对一台正在运行的 pfSense。每次调用都是读取操作。如果有
    任何问题,请告诉我们。

## 监控内容 {#what-it-watches}

所有指标都以 `dumbmonit_pfsense_` 为前缀。

| 指标 | 标签 | 含义 |
|---|---|---|
| `cpu_usage_percent`, `memory_used_percent`, `swap_used_percent`, `disk_used_percent`, `mbuf_used_percent` | | 与控制台显示的一致,以百分比表示。CPU 数值是 pfSense 自己根据负载平均值做出的估算。 |
| `temperature_celsius` | | 当 pfSense 能够读取某个传感器时。 |
| `load_average` | `period`:`1m`、`5m`、`15m` | 负载平均值。 |
| `cpu_count` | | 处理器核心数。 |
| `version_info` | `version` | 固定值 1:pfSense 版本,`2.7.2-RELEASE`。 |
| `gateway_up` | `gateway` | 当 pfSense 把该网关标记为宕机时为 0,否则为 1。 |
| `gateway_status_info` | `gateway`, `status` | 固定值 1:pfSense 自身的状态词(一切正常时为 `none`,`down`、`loss`、`delay`)。 |
| `gateway_loss_percent` | `gateway` | 到监控地址的丢包率。 |
| `gateway_delay_milliseconds`, `gateway_stddev_milliseconds` | `gateway` | 到监控地址的往返时间及其偏差;网关宕机期间不存在。 |
| `gateways`, `gateways_down` | | 计数。 |
| `interface_enabled`, `interface_up` | `interface`, `descr` | 在配置中已启用;链路已建立。 |
| `interface_down` | `interface`, `descr` | 当接口已启用且没有链路时为 1。已禁用的接口永不会被视为宕机。 |
| `interface_bytes_in_total`, `interface_bytes_out_total`, `interface_errors_in_total`, `interface_errors_out_total` | `interface`, `descr` | 计数器。 |
| `interfaces_down` | | 计数。 |
| `service_running`, `service_enabled` | `service`, `description` | 与 Status > Services 显示的一致。 |
| `service_stopped` | `service`, `description` | 当该服务已启用但未运行时为 1。 |
| `services_stopped` | | 计数。 |
| `states`, `states_max`, `states_used_percent` | | pf 跟踪的连接数、上限以及已用比例。需要 `/api/v2/firewall/states/size GET` 权限。 |
| `carp_enabled`, `carp_maintenance_mode` | | CARP 已启用时为 1；被置于持久维护模式时为 1。需要 `/api/v2/status/carp GET` 权限。 |
| `restapi_update_available` | | 配合 `/api/v2/system/restapi/version` 权限:当该 REST API 插件有新版本时为 1。 |
| `restapi_version_info` | `version`, `latest` | 固定值 1:该插件已安装的版本和最新版本。 |

pfSense 不会通过这个 API 说明是否有 pfSense 本身的更新可用:唯一
涉及系统更新的接口(`/api/v2/system/update`)会直接启动更新。
DumbMonit 只报告该插件自身的更新。

状态表和 CARP 是可选读取。每一项都需要为 `dumbmonit` 用户再添加一个权限：`/api/v2/firewall/states/size GET` 和 `/api/v2/status/carp GET`。没有相应权限时，pfSense 会拒绝该读取，对应的数值只是不存在：没有错误，也没有告警。

适用的[内置规则](../alerting/rules.md#pfsense)：

| 规则 | 什么 | 默认阈值 | 保持 | 严重级别 | 提醒 |
|---|---|---|---|---|---|
| pfSense 网关宕机 | pfSense 网关宕机:其监控地址不再响应,所承载的流量中断或切换(`dumbmonit_pfsense_gateway_up`)。 | < 1 | 3 分钟 | 警告 (`critical`) | 6 小时 |
| pfSense 网关丢包 | pfSense 网关对其监控地址的探测丢失超过 10 %:线路质量下降(`dumbmonit_pfsense_gateway_loss_percent unless on (target, gateway) dumbmonit_pfsense_gateway_up == 0`)。 | > 10 % | 10 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense 网关缓慢 | 到 pfSense 网关监控地址的往返时间已连续十五分钟超过 500 ms(`dumbmonit_pfsense_gateway_delay_milliseconds`)。 | > 500 ms | 15 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense 接口无链路 | 已启用的 pfSense 接口没有链路:网线、交换机端口或网卡(`dumbmonit_pfsense_interface_down`)。 | > 0 | 5 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense 服务已停止 | 已启用的 pfSense 服务未运行:DNS 解析器、VPN、NTP…(`dumbmonit_pfsense_service_stopped`)。 | > 0 | 5 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense 磁盘几乎已满 | pfSense 磁盘已满超过 90 %:日志、报告和软件包更新将无法写入(`dumbmonit_pfsense_disk_used_percent`)。 | > 90 % | 30 分钟 | 通知 (`warning`) | 1 天 |
| pfSense 状态表几乎已满 | pf 跟踪的连接数超过其上限的 80 %：超过上限后，防火墙会丢弃新连接，且没有任何其他症状。低于 70 % 时解除(`dumbmonit_pfsense_states_used_percent`)。 | > 80 % | 10 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense 网络缓冲区几乎耗尽 | 超过 90 % 的网络缓冲区已被占用：缓冲区耗尽的防火墙会停止转发，而此时处理器仍然空闲。低于 80 % 时解除(`dumbmonit_pfsense_mbuf_used_percent`)。 | > 90 % | 10 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense 温度高 | pfSense 读到的温度连续十分钟高于 85 °C：设备即将自行降速。降到 78 °C 时解除(`dumbmonit_pfsense_temperature_celsius`)。 | > 85 °C | 10 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense CPU 高 | 处理器连续十五分钟高于 90 %。低于 80 % 时解除(`dumbmonit_pfsense_cpu_usage_percent`)。 | > 90 % | 15 分钟 | 通知 (`warning`) | 6 小时 |
| pfSense 交换空间被使用 | pfSense 使用超过一半的交换空间已达一小时：内存不足(`dumbmonit_pfsense_swap_used_percent`)。 | > 50 % | 1 小时 | 通知 (`warning`) | 24 小时 |
| pfSense 接口错误 | 某个接口在十五分钟内记录了超过 100 个接收或发送错误：请检查网线、SFP 模块和双工设置(`dumbmonit_pfsense_interface_errors_in_total` + `dumbmonit_pfsense_interface_errors_out_total`)。 | > 100 | 15 分钟 | 通知 (`warning`) | 24 小时 |
| pfSense CARP 维护模式一直开启 | CARP 处于持久维护模式已达一小时：这台防火墙已把虚拟地址交给对端，不会自行收回(`dumbmonit_pfsense_carp_maintenance_mode`)。 | > 0 | 1 小时 | 信息 (`info`) | 24 小时 |

## 监控项页面 {#the-device-page}

Firewall 面板会用一句话说明哪里出了问题,显示 CPU、内存、磁盘
和温度,然后列出每个网关及其延迟和丢包率、无链路的接口、已停止
的服务,以及该 REST API 插件的可用更新。

## 为 DumbMonit 创建一个只读的 REST API 密钥 {#create-a-read-only-rest-api-key-for-dumbmonit}

1. 从 Diagnostics > Command Prompt 或 SSH shell 安装社区 REST API 插件(pfSense-pkg-RESTAPI,版本 2),使用项目发布页面中与你的 pfSense 版本匹配的文件(下面以 2.7.2 为例)。然后在 System > REST API > Settings 下,在认证方式中勾选 BasicAuth 和 Key 两项。

    ```
    pkg-static add https://github.com/pfrest/pfSense-pkg-RESTAPI/releases/latest/download/pfSense-2.7.2-pkg-RESTAPI.pkg
    ```

2. 在 System > User Manager 下,添加一个名为 dumbmonit、密码为较长随机字符串的用户。在 Effective Privileges 下,只添加以下 REST API 权限:/api/v2/status/system GET、/api/v2/status/gateways GET、/api/v2/status/interfaces GET、/api/v2/status/services GET、/api/v2/system/version GET,再加上 /api/v2/system/restapi/version GET 以便获知该插件自身的更新。仅为下一步,再额外添加 /api/v2/auth/key POST。

3. 以 dumbmonit 身份,从任何能访问 pfSense 的机器上创建密钥:该密钥恰好携带该用户的权限。复制应答中的密钥值,然后再次移除 /api/v2/auth/key POST 权限。

    ```
    curl -k -u dumbmonit -X POST -H 'Content-Type: application/json' -d '{"descr":"DumbMonit"}' https://pfsense.lan/api/v2/auth/key
    ```

4. 在 DumbMonit 中,输入 pfSense 的地址,例如 "pfsense.lan",并把该密钥粘贴为 API 密钥。pfSense 自带自签名证书:除非你替换过它,否则请勾选"接受无法验证的证书"。

!!! warning "警告"
    该 REST API 不会说明 pfSense 本身是否有更新,只会说明该插件
    本身是否有更新。该插件是一个社区项目,不属于 pfSense 本身:
    请在每次 pfSense 升级后重新安装它。

## 凭据 {#credentials}

| 凭据 | 字段 |
|---|---|
| REST API 密钥 | 在第 3 步中为 `dumbmonit` 创建的密钥,以 `X-API-Key` 发送。 |

地址:pfSense 的主机名或 IP、`host:port`,或 URL。选项:

- **协议**(`https`)和**端口**(443):webConfigurator 所使用的,
  在 System > Advanced > Admin Access 下设置。
- **接受无法验证的证书**:pfSense 自身的证书是自签名的。
- **每次请求的超时时间**(15 秒)。

## 已知限制

- 不读取证书：该软件包会把私钥连同证书一并返回，DumbMonit 拒绝请求它们。请在提供该证书的地址上用 [TLS 监控项](services.md#tls) 跟踪到期日期。
- VPN 隧道（IPsec、OpenVPN、WireGuard）不会逐条跟踪。它们的服务会被跟踪，由“pfSense 服务已停止”负责。
- 不报告 pfSense 的更新：API 中唯一涉及更新的路由会直接启动一次更新。
- 内存没有规则：pfSense 把 ZFS 缓存算作已用内存，因此数值偏高是正常的。交换空间被使用才是防火墙内存不足的可靠信号。

## 故障排查 {#troubleshooting}

"pfSense refused the API key (401)":密钥错误,或在 System >
REST API > Settings 下的认证方式中没有勾选 Key。

"pfSense accepted the API key but refused … (403)":`dumbmonit`
用户缺少该页面所需的权限;错误消息会指明是哪一项。

"… not found: is the REST API package installed?":该插件缺失,
通常是在 pfSense 升级后被移除了。请按第 1 步重新安装它。
