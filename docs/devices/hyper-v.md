# Microsoft Hyper-V

A Hyper-V host monitored without installing anything, through WinRM
(WS-Management) and read-only WMI queries on the `root\virtualization\v2`
namespace: host CPU, memory and uptime, and for each virtual machine its state,
health, uptime, CPU, assigned memory, integration services, replication and
checkpoints.

DumbMonit only sends fixed `SELECT` queries (WQL `Enumerate`). It never runs a
command or a script on the host, and it can go through a
[relay agent](agent.md) to reach a network the server cannot. If the
[Windows agent](agent.md) is already installed on the host, it can also read the
Hyper-V performance counters (`hyperv_*` series); this monitor needs no agent
and adds checkpoints, replication and integration services.

!!! warning "Beta"

    Beta: written from Microsoft's WMI documentation, not yet tested on every
    Windows Server version. The account runs no command on the host, it only
    reads WMI. NTLM and Basic (HTTPS only) are supported, Kerberos is not. A
    self-signed certificate needs the option Accept an unverifiable certificate.

## Network ports to open

From the DumbMonit server (or its relay agent) to the device:

| Port | Protocol | Purpose |
|---|---|---|
| 5986 | TCP | WinRM over HTTPS (default) |
| 5985 | TCP | Only with the Allow unencrypted traffic option |

## Prepare the Hyper-V host

1. On the host, in an elevated PowerShell, enable WinRM.

    ```powershell
    Enable-PSRemoting -SkipNetworkProfileCheck -Force
    ```

2. Create the HTTPS listener with a certificate named after the host (use your own certificate instead of this self-signed one if you have one), then open its port.

    ```powershell
    $cert = New-SelfSignedCertificate -DnsName "hv01.corp.example.com" -CertStoreLocation Cert:\LocalMachine\My
    New-Item -Path WSMan:\LocalHost\Listener -Transport HTTPS -Address * -CertificateThumbPrint $cert.Thumbprint -Force
    New-NetFirewallRule -DisplayName "WinRM HTTPS" -Direction Inbound -Protocol TCP -LocalPort 5986 -Action Allow
    ```

3. Create the account with a long random password, then add it to the groups listed below.

    ```powershell
    net user dumbmonit * /add
    net localgroup "Remote Management Users" dumbmonit /add
    net localgroup "Performance Monitor Users" dumbmonit /add
    ```

4. Give the account read access to the Hyper-V WMI namespace: wmimgmt.msc → right-click WMI Control → Properties → Security → Root → virtualization → v2 → Security, add it and tick Enable Account and Remote Enable. Do the same on Root → CIMV2 for the host CPU and memory.

5. In DumbMonit, enter the host name as its certificate names it (hv01.corp.example.com) and log in as dumbmonit, or DOMAIN\dumbmonit for a domain account.

### Permissions

| Right | Used for |
|---|---|
| Remote Management Users | Allows the WinRM login |
| Performance Monitor Users | Reads the hypervisor processor load |
| WMI root/virtualization/v2: Enable Account, Remote Enable | Reads the virtual machines, their checkpoints and replication |
| WMI root/cimv2: Enable Account, Remote Enable | Reads the host memory and uptime |

The account does not belong to the Hyper-V Administrators group: it can see the
virtual machines but cannot start, stop, change or delete any of them. If the
WMI permissions alone are not enough on your version of Windows, adding the
account to Hyper-V Administrators works, at the price of full control.

See [Windows without an agent](winrm.md) for the authentication modes
(NTLM, Basic over HTTPS), unencrypted HTTP, certificates and Group Policy
roll-out: they are the same.

## What it watches

All metrics are prefixed `dumbmonit_hyperv_`. Per-VM series carry the label
`vm` (the VM name; two VMs with the same name get the start of their identifier
appended).

| Metric | Labels | Meaning |
|---|---|---|
| `vms`, `vms_running` | | Virtual machines on the host, and those running. |
| `host_cpu_percent` | | Processor load of the hypervisor (all logical processors). |
| `host_memory_total_bytes`, `host_memory_used_bytes`, `host_memory_used_percent` | | Host memory. |
| `host_uptime_seconds` | | Time since the host started. |
| `vm_state` | `vm` | The Hyper-V state code: 2 running, 3 off, 32768 paused, 32769 saved, 32770 starting, 32774 stopping. |
| `vm_running` | `vm` | 1 when the VM runs. A VM that is off is not an alert: an administrator may have shut it down. |
| `vm_health_ok` | `vm` | 1 when Hyper-V reports the VM healthy. |
| `vm_uptime_seconds` | `vm` | Running VMs only. |
| `vm_cpu_percent`, `vm_cpus` | `vm` | Processor load of the VM, and its virtual processors. |
| `vm_memory_assigned_bytes` | `vm` | Memory assigned to a running VM (the current value with Dynamic Memory). |
| `vm_integration_ok` | `vm` | 1 when the guest answers the heartbeat integration service of a running VM, 0 when there is no contact. Absent when the service state is unknown. |
| `vm_replication_ok` | `vm` | Only for replicated VMs: 1 when the replication health is Normal, 0 for Warning or Critical. |
| `vm_checkpoints`, `vm_checkpoint_oldest_age_seconds` | `vm` | Checkpoints of the VM, and the age of the oldest one. |
| `winrm_section_readable` | `section` | 0 when an optional read (`hyperv_summary`, `hyperv_checkpoints`, `hyperv_host_cpu`, `hyperv_os`) is refused: that series is then missing. |

Only the list of virtual machines is mandatory. A host without the Hyper-V role,
or an account without access to the namespace, is shown as a configuration error
and does not raise an outage.

The [built-in rules](../alerting/rules.md) that apply: Hyper-V VM unhealthy,
Hyper-V VM integration services not answering, Hyper-V replication unhealthy,
Hyper-V VM checkpoint too old (3 days), Hyper-V host CPU saturated and Hyper-V
host memory almost full, plus Monitor unreachable.

## Options

| Option | Default | Role |
|---|---|---|
| Allow unencrypted traffic | off | HTTP on 5985 instead of HTTPS on 5986 |
| Port | 5986 | Used if the address does not give one |
| Authentication | `ntlm` | `ntlm` or `basic` (HTTPS only) |
| Accept an unverifiable certificate | off | For a self-signed listener |
| Timeout per request | 15 s | 1 to 120 |
