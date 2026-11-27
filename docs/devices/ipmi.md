# Server hardware (IPMI)

The BMC of a server (Supermicro, iDRAC, iLO, XClarity, or any IPMI-capable
controller), read over IPMI v2.0: temperatures, fans, voltages, power supplies,
chassis power state and event log.

!!! warning "Beta — validated against the specification only, not yet tested on a real BMC"

    This integration was built from the IPMI v2.0 specification and an
    open-source pure-Rust client library, not against real hardware. It only
    reads. Tell us what breaks. If your BMC serves Redfish, the
    [Redfish](redfish.md) integration is richer and better tested.

DumbMonit opens an RMCP+ session over UDP 623 (IPMI v2.0, falling back to
v1.5 if the BMC offers nothing else), reads the data and drops the session. The
protocol is implemented inside DumbMonit, in Rust: no `ipmitool` or any other
program is run on the server or on the relay agent. Only reads are issued
(chassis status, sensor readings, SDR repository, SEL information); nothing is
ever written to the BMC.

## Network ports to open

From the DumbMonit server (or its relay agent) to the device:

| Port | Protocol | Purpose |
|---|---|---|
| 623 | UDP | IPMI over LAN (RMCP+) |

## Prepare the BMC for DumbMonit

1. In the BMC web interface, create a user for DumbMonit with the privilege level User (read-only) and IPMI over LAN enabled.

2. Make sure IPMI over LAN is enabled on the BMC network interface (often under Configuration > Network > IPMI, or Services).

3. Allow UDP 623 from the DumbMonit server (or its relay agent) to the BMC.

4. In DumbMonit, enter the BMC address and the user name and password.

!!! warning
    IPMI v2.0 has known protocol weaknesses (password hashes can be captured on
    the wire): keep the BMC on a management network and use a dedicated user
    with a long random password (20 characters at most).

## Credentials

| Credential | Fields |
|---|---|
| BMC user | User name and password. Stored encrypted, never shown again. |

Address: the BMC host name or IP, optionally with a port (`bmc.lan:623`).
Option: **Timeout per exchange** (5 s).

## What it watches

All metrics are prefixed `dumbmonit_ipmi_`.

| Metric | Labels | Meaning |
|---|---|---|
| `temperature_celsius`, `fan_rpm`, `voltage_volts`, `current_amps`, `power_watts` | `sensor` | Analog sensors of the SDR repository, by unit. |
| `sensor_value` | `sensor`, `unit` | Analog sensors in another unit. |
| `sensor_ok` | `sensor` | 0 when the sensor is past a critical or non-recoverable threshold. |
| `sensors`, `sensors_out_of_limits` | | Count of analog sensors, and those past a critical threshold. |
| `psu_present`, `psu_ok` | `psu` | Power supply sensors: presence, and 0 on a fault, a lost input or a predicted failure. |
| `psus_faulty` | | Power supplies in fault. |
| `chassis_power_on`, `chassis_power_fault`, `chassis_power_overload` | | Chassis power state. |
| `sel_entries`, `sel_overflow` | | Entries in the System Event Log, and whether it overflowed. |
| `read_truncated` | | 1 when the read of the sensors was cut by the 45 s limit. |

At most 200 analog sensors and 200 power supplies are read. The event log
itself is counted, not decoded.

The [built-in rules](../alerting/rules.md#ipmi) that apply:

- **IPMI power supply fault**: a faulty power supply for 5 minutes (Critical).
- **IPMI sensor out of limits**: a sensor past its critical threshold for 10
  minutes (Warning).

## Troubleshooting

"The BMC does not answer on UDP 623": IPMI over LAN is disabled on the BMC, the
port is filtered, or the address is not the BMC's (the BMC usually has its own
IP, not the server's).

"The BMC refused the session": wrong user name or password, a user without IPMI
over LAN access, or a privilege level below User. Some BMCs disable
RMCP+ ciphers that the library offers; enable cipher suite 3 or 17.

Sensors are missing: the BMC does not expose them in its SDR repository, or
they are discrete sensors other than power supplies (not read).
