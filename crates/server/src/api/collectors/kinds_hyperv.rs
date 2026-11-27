//! Notice du type Hyper-V (lu par WinRM, sans agent).

use super::*;

/// La notice d'un type de cette famille, `None` pour tout autre type.
pub(super) fn view(kind: &str) -> Option<CollectorView> {
    Some(match kind {
        "hyperv" => CollectorView {
            kind: "hyperv",
            label: "Microsoft Hyper-V",
            summary: "Beta. A Hyper-V host without installing anything, through WinRM and WMI queries: host CPU, memory and uptime, and for each virtual machine its state, health, uptime, CPU, assigned memory, integration services, replication and checkpoints.",
            examples: &[
                "Windows Server 2019 with Hyper-V",
                "Windows Server 2022 with Hyper-V",
                "Hyper-V Server",
            ],
            credential_types: &["username_password"],
            credentials: &[WINRM_LOGIN],
            address_hint: "hv01.corp.example.com",
            default_port: 5986,
            setup: Setup {
                title: "Give DumbMonit a read-only WinRM account on the Hyper-V host",
                steps: &[
                    "On the host, in an elevated PowerShell, enable WinRM.\nEnable-PSRemoting -SkipNetworkProfileCheck -Force",
                    "Create the HTTPS listener with a certificate named after the host (use your own certificate instead of this self-signed one if you have one), then open its port.\n$cert = New-SelfSignedCertificate -DnsName \"hv01.corp.example.com\" -CertStoreLocation Cert:\\LocalMachine\\My\nNew-Item -Path WSMan:\\LocalHost\\Listener -Transport HTTPS -Address * -CertificateThumbPrint $cert.Thumbprint -Force\nNew-NetFirewallRule -DisplayName \"WinRM HTTPS\" -Direction Inbound -Protocol TCP -LocalPort 5986 -Action Allow",
                    "Create the account with a long random password, then add it to the groups listed below.\nnet user dumbmonit * /add\nnet localgroup \"Remote Management Users\" dumbmonit /add\nnet localgroup \"Performance Monitor Users\" dumbmonit /add",
                    "In wmimgmt.msc, open WMI Control → Properties → Security → Root → virtualization → v2 → Security, add the account and tick Enable Account and Remote Enable.",
                    "Do the same on Root → CIMV2, for the host CPU and memory.",
                    "In DumbMonit, enter the host name as its certificate names it (hv01.corp.example.com) and log in as dumbmonit, or DOMAIN\\dumbmonit for a domain account.",
                ],
                ports: &[
                    to_device!(5986, "tcp", "WinRM over HTTPS (default)"),
                    to_device_optional!(
                        5985,
                        "tcp",
                        "Only with the Allow unencrypted traffic option"
                    ),
                ],
                permissions: &[
                    "Remote Management Users\nAllows the WinRM login",
                    "Performance Monitor Users\nReads the hypervisor processor load",
                    "WMI root/virtualization/v2: Enable Account, Remote Enable\nReads the virtual machines, their checkpoints and replication",
                    "WMI root/cimv2: Enable Account, Remote Enable\nReads the host memory and uptime",
                ],
                warning: "Beta: written from Microsoft's WMI documentation, not yet tested on every Windows Server version. The account runs no command on the host, it only reads WMI. NTLM and Basic (HTTPS only) are supported, Kerberos is not. A self-signed certificate needs the option Accept an unverifiable certificate.",
            },
            options: HYPERV_OPTIONS,
        },
        _ => return None,
    })
}
