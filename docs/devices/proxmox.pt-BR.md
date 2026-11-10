# Proxmox VE {#proxmox-ve}

Hipervisor: nós com seus daemons e rede, máquinas virtuais, contêineres,
storages, pools LVM thin, backups, alta disponibilidade, snapshots,
replicação, Ceph até o OSD, discos físicos e pools ZFS, atualizações
pendentes, mudanças de pacote, assinatura e certificados. Um monitor
pode cobrir um cluster inteiro.

## O painel de nós {#the-nodes-panel}

A página do monitor abre com um cartão por nó: CPU, memória e
sistema de arquivos raiz como três medidores, o tempo de atividade e a
versão `pve-manager` instalada naquele nó, depois só o que está errado —
os daemons que não estão em execução, as interfaces declaradas na
inicialização que não estão ativas — e a ocupação de cada pool thin e
grupo de volume LVM. Um nó que parou de responder mantém seu cartão e lê
*offline*.

Duas linhas sob os medidores, quando o nó as reporta: a parcela de
tempo de CPU gasta esperando por armazenamento (**I/O wait** — um nó em
10% de CPU e 30% de I/O wait não tem margem alguma, e nenhum outro
número diz isso) e a memória que o **KSM** recuperou compartilhando
páginas idênticas entre guests, que é como um nó hospeda mais que sua
RAM.

Mais duas entradas se juntam à lista de "o que está errado", ambas em
palavras:

* **Reinício necessário** — um kernel mais novo está instalado que o
  que está em execução. O cartão nomeia ambas as versões, já que o nó
  continua executando o antigo até ser reiniciado. Isso vem de
  `/nodes/{node}/apt/versions`, que só precisa de `Sys.Audit`;
* **Atualizações pendentes** — quantos pacotes Proxmox têm uma versão
  candidata mais nova, da mesma chamada. Não é a mesma coisa que
  `apt/update`, que conta todo pacote pendente do sistema mas precisa
  de `Sys.Modify` e geralmente é recusado.

Quando o cluster reporta um watchdog HA (Proxmox VE 9 e posterior), seu
estado é declarado ao lado do título do painel: *HA fencing armado* uma
vez que recursos são entregues ao HA, *HA fencing em espera* enquanto
nenhum está — nesse caso nenhum nó será fenced, o que é normal mas vale
a pena saber antes de contar com isso.

Um cluster atualizado nó por nó mostra isso aqui: `/version` responde
pelo nó que foi perguntado, então cada cartão carrega sua própria
versão e um atrasado é visível sem abrir um shell.

Os dados vêm de `GET /api/targets/{id}/proxmox/nodes`, lido da última
sondagem no armazenamento de métricas.

## O painel de guests {#the-guests-panel}

Abaixo dos nós, o painel **Guests**: uma linha por máquina virtual e
contêiner, agrupados por nó, que responde as perguntas que um
administrador faz primeiro — está rodando, está ocupado, seu disco está
cheio, quando foi o último backup. Cada linha mostra:

| Coluna | Fonte | Notas |
|---|---|---|
| Status | palavra + placa: rodando, parado, pausado, suspenso, template | Um template é listado mas nunca conta como "parado". |
| Nome, VMID, nó | o inventário | |
| CPU | `guest_cpu_percent`, dos núcleos alocados ao guest | Uma barra mais o número de núcleos. |
| Memória | `guest_memory_used_bytes` / `guest_memory_total_bytes` | O tamanho do balloon quando a VM tem um. Uma segunda linha, *+N no host*, aparece quando `guest_memory_host_bytes` está significativamente acima do que o guest vê: um processo QEMU custa mais ao hipervisor que a memória que entrega, e esse excedente é o overhead de emulação. Abaixo de 64 MiB ou 5% a linha fica fora. |
| Disco | `guest_disk_used_bytes` / `guest_disk_total_bytes` | Contêineres: uso real. VMs: tamanho sempre; uso só quando o agente QEMU responde (veja abaixo), senão "apenas tamanho". |
| Rede | `rate(guest_network_in/out_bytes)` nas últimas sondagens | |
| Tempo de atividade | `guest_uptime_seconds` | |
| Último backup | `backup_last_age_seconds` | Vazio quando nenhum backup é conhecido. |
| HA | `ha_resource_state_info` | Apenas para guests sob alta disponibilidade. |
| Bloqueio | `guest_locked` | Um badge, só enquanto um bloqueio é mantido (`backup`, `migrate`, `snapshot`…). |

A lista ordena por qualquer coluna, filtra por nó, status ou pool, e
cada grupo de nó se recolhe. Clicar em uma linha abre sua CPU e memória
no intervalo de tempo da página, junto com o sistema operacional e os
endereços IP vistos de dentro do guest. Os dados vêm de
`GET /api/targets/{id}/proxmox/guests`, que lê a última sondagem no
armazenamento de métricas — nada é pedido ao hipervisor.

Um bloqueio é normal pelos poucos minutos que um backup, uma migração
ou um snapshot levam. Deixado por um job interrompido, ele bloqueia
toda operação naquele guest — incluindo o backup da noite seguinte —
até `qm unlock` (ou `pct unlock`) o limpar, por isso "Guest locked"
dispara depois de seis horas.

O inventário vem de `/cluster/resources`, o próprio cache do cluster,
então os guests de um nó que parou de responder ainda são listados —
com o status `unknown` e seus tamanhos, sem medições em tempo real — em
vez de desaparecer do painel no momento em que você quer olhar. Também
custa duas chamadas de API menos por nó do que percorrer
`/nodes/{node}/qemu` e `/nodes/{node}/lxc`, que é o que a opção
`cluster_resources` recorre quando desligada.

### Uso de disco dentro de uma VM: o agente QEMU guest {#disk-usage-inside-a-vm-the-qemu-guest-agent}

O Proxmox vê o disco de uma VM como um volume opaco: `disk` é sempre 0
para uma VM, só seu tamanho é conhecido. Para ler o uso de dentro, o
coletor pede ao agente QEMU guest (`agent/get-fsinfo`) para toda VM em
execução cuja configuração ativa o agente (Options → QEMU Guest Agent).
Duas condições:

* `qemu-guest-agent` instalado e em execução dentro da VM (Debian e
  Ubuntu: `apt install qemu-guest-agent`; Windows: a ISO de drivers
  VirtIO);
* o token tem `VM.GuestAgent.Audit` na VM — `VM.Monitor` antes do
  Proxmox VE 9 (o papel `DumbMonit` abaixo o concede em `/`).

O sistema de arquivos raiz (`/`, ou `C:\` no Windows; na falta disso, o
maior) dá `guest_disk_used_bytes` e `guest_disk_used_percent`, a mesma
série que um contêiner tem, então a regra "VM or container disk almost
full" cobre ambos. Todo sistema de arquivos real também recebe
`guest_fs_total/used_bytes` e `guest_fs_used_percent` com um rótulo
`mountpoint`; sistemas de arquivos pseudo (tmpfs, squashfs, montagens
snap) e mídia sem capacidade são pulados. `guest_agent_enabled` diz se
a configuração da VM ativa o agente, `guest_agent_running` se ele
respondeu. Sem o agente ou o direito, a VM mantém seu tamanho e
simplesmente não tem uso — nunca um "0%" enganoso.

### O sistema operacional e endereços de um guest {#the-operating-system-and-addresses-of-a-guest}

O Proxmox só conhece o tipo de SO escrito na configuração de um guest
(`l26`, `win11`), não o que realmente roda. Com a opção `guest_os`
ativada, o coletor pergunta a cada guest em execução o que é:
`agent/get-osinfo` e `agent/network-get-interfaces` para uma VM,
`/nodes/{node}/lxc/{vmid}/interfaces` para um contêiner, que não
precisa de nada instalado dentro. Isso dá `guest_os_info` (`Debian
GNU/Linux 12 (bookworm)`, seu id, sua versão, o kernel) e até quatro
`guest_ip_info` — endereços loopback e IPv6 link-local são pulados, e
uma máquina com dezenas de interfaces virtuais não enche o banco de
dados de séries.

Ambos são perguntados **no máximo uma vez por hora por guest** e
republicados da memória no intervalo: um sistema operacional não muda
entre duas sondagens, e um endereço quase nunca muda. Em um cluster de
cinquenta máquinas em execução, essa é a diferença entre duas chamadas
extras por sondagem e duas por hora.

Essas duas chamadas de agente precisam de `VM.GuestAgent.Audit`. O
Proxmox VE 7 não tinha isso e usava `VM.Monitor`; o Proxmox VE 9 removeu
`VM.Monitor` e mantém só os privilégios de agente guest. Sem o que sua
versão conhece, as duas séries simplesmente ficam ausentes; nada mais
muda.

## O painel Ceph {#the-ceph-panel}

Uma seção **Ceph** aparece na página do monitor só quando o cluster
realmente tem Ceph: saúde e as palavras detrás dela, capacidade total e
usada, quantos OSDs estão up e in, uma tabela de OSDs (host, classe de
monitor, uso, latência de apply e commit) e uma tabela de pools
(uso, tamanho e tamanho mínimo, grupos de posicionamento contra o
número que o autoscaler escolheria), os sistemas de arquivos CephFS, as
flags de OSD deixadas definidas e as verificações de saúde atualmente
silenciadas.

Essas duas últimas são as que um painel geralmente esconde. `noout`
definido para uma manutenção e nunca limpo significa que o Ceph não vai
rebalancear um OSD morto de novo, e uma verificação silenciada não
aparece mais em `HEALTH_OK` — em ambos os casos o cluster parece
saudável enquanto sua redundância silenciosamente desaparece.

`GET /api/targets/{id}/proxmox/ceph` responde `available: false`
quando o cluster não tem Ceph, que é como a interface sabe que não deve
desenhar a seção de forma alguma. O detalhe custa três chamadas em um
único nó mais duas em todo o cluster, qualquer que seja o tamanho do
cluster, e precisa da opção `ceph_detail` além de `ceph`.

## O que ele observa {#what-it-watches}

Todas as métricas têm o prefixo `dumbmonit_proxmox_`.

| Família | Métricas | Rótulos |
|---|---|---|
| Cluster | `cluster_quorate`, `cluster_nodes`, `cluster_nodes_online`, `cluster_member_online`; `version_info` (+ `version`, `release`, `repoid`) | `cluster`, `node` |
| Inventário do cluster | `cluster_guests_total`, `cluster_guests_running`, `cluster_templates_total` (templates contados separadamente, nunca como "parado"); `pool_guests`, um por pool de recursos | `pool` |
| Nós | `node_up`, `node_cpu_percent`, `node_cpu_count`, `node_load1/5/15`, `node_memory_used/total_bytes`, `node_memory_percent`, `node_swap_used/total_bytes`, `node_rootfs_used/total/avail_bytes`, `node_rootfs_percent`, `node_cpu_iowait_percent` (tempo gasto esperando por discos — um nó em 10% de CPU e 30% de iowait não tem margem), `node_ksm_shared_bytes` (memória recuperada por deduplicação de página), `node_uptime_seconds`, `node_version_info` | `node` |
| Serviços de nó | `node_service_running`, `node_service_state_info` (+ `state`), `node_services_failed`, `node_core_services_down`; `node_pve_version_info` (+ `version`, `release`) para o nó que respondeu | `node`, `service` |
| Rede de nó | `node_interface_active`, `node_interface_exists`, `node_interface_autostart`, `node_interface_mtu`, `node_interface_offline`; total `node_interfaces_offline` | `node`, `iface`, `type` (`bridge`, `bond`, `vlan`, `eth`…) |
| LVM, pools thin e montagens | `node_lvm_vg_size/free_bytes`, `node_lvm_vg_used_percent`, `node_lvm_vg_physical_volumes` (rótulo `vg`); `node_thinpool_size_bytes`, `node_thinpool_used_bytes`, `node_thinpool_used_percent`, `node_thinpool_metadata_size_bytes`, `node_thinpool_metadata_used_percent` (rótulos `vg`, `pool`); `node_directory_mount_info` (+ `path`, `device`, `fstype`), `node_directory_mounts` | `node`, `vg`, `pool` |
| Guests (VMs e contêineres) | `guest_status_info` (+ `status`: `running`, `stopped`, `paused`, `suspended`, `template`), `guest_running` (não para templates), `guest_cpu_percent` (dos núcleos alocados), `guest_cpu_count`, `guest_memory_used/total_bytes`, `guest_memory_host_bytes` (Proxmox VE 9: a memória que a VM toma no host, overhead de emulação incluído), `guest_memory_percent` (do próprio ponto de vista do guest; no Proxmox VE 9 vem do driver balloon, e uma VM sem um tem bytes mas não porcentagem em vez de um número acima de 100%), `guest_disk_total_bytes`, `guest_disk_used_bytes` e `guest_disk_used_percent` (contêineres; VMs apenas através do agente guest), `guest_disk_read/write_bytes`, `guest_network_in/out_bytes` (contadores), `guest_uptime_seconds`; `guest_pool_info` (+ `pool`), `guest_locked` (+ `lock`; a série existe só enquanto o bloqueio é mantido, então sua duração se lê direto do gráfico) | `node`, `vmid`, `name`, `type` (`qemu` ou `lxc`) |
| Detalhe de VM (VMs em execução) | `guest_agent_enabled`, `guest_agent_running`, `guest_balloon_bytes`, `guest_memory_guest_free_bytes` (memória livre vista de dentro, driver balloon), `guest_running_qemu_info` (+ `version`: o QEMU com que a VM foi iniciada — depois de uma atualização do hipervisor permanece atrás até a VM ser parada e iniciada de novo, o que uma migração em tempo real não faz); por sistema de arquivos `guest_fs_total_bytes`, `guest_fs_used_bytes`, `guest_fs_used_percent` | `node`, `vmid`, `name`, `type`; sistemas de arquivos: + `mountpoint`, `fstype` |
| Sistema e endereços do guest | `guest_os_info` (+ `os`, `os_id`, `os_version`, `kernel`), `guest_ip_info` (+ `iface`, `ip`; quatro endereços roteáveis por guest no máximo). Atualizado uma vez por hora | `node`, `vmid`, `name`, `type` |
| Placas de rede do guest | `guest_netdev_in_bytes`, `guest_netdev_out_bytes` (contadores), um par por interface virtual onde `guest_network_in/out_bytes` só dá o total do guest | `node`, `vmid`, `dev` (`tap100i0`, `veth200i0`) |
| Storages | `storage_active`, `storage_enabled`, `storage_used/total/avail_bytes`, `storage_used_percent` (capacidade é publicada só quando o Proxmox reporta uma: um storage inativo, ou um datastore PBS cujo tamanho o PVE não lê, não tem nenhuma em vez de zero bytes) | `node`, `storage`, `type` (`dir`, `lvmthin`, `zfspool`, `nfs`, `pbs`…), `shared` |
| Backups (por guest) | `backup_present`, `backup_last_timestamp_seconds`, `backup_last_age_seconds`, `backup_last_size_bytes`, `backup_count`; totais `backup_guests_total`, `backup_guests_without_backup` | `vmid`, `name`, `node`, `type` |
| Backups (execuções vzdump, por nó) | `backup_job_runs`, `backup_job_failures`, `backup_job_last_ok`, `backup_job_last_timestamp_seconds`, `backup_job_last_age_seconds`, `backup_job_last_duration_seconds` | `node` |
| Jobs de backup (agendados) | `backup_job_enabled`, `backup_job_next_run_seconds`, `backup_job_last_ok`, `backup_job_last_run_age_seconds` (os dois últimos só quando uma tarefa `vzdump` carrega o id do job, PVE 7.2+); totais `backup_jobs_total`, `backup_guests_not_covered`; `backup_covered` por guest (0 para um guest que nenhum job cobre) | `job`, `storage`, `schedule`; `backup_covered`: `vmid`, `name`, `node`, `type` |
| Cobertura de job de backup | `backup_job_guests`, `backup_job_volumes_included`, `backup_job_volumes_excluded`; `backup_job_guest_excluded_volumes` (+ `vmid`, `name`, `type`, `reason`), publicado só para uma exclusão *inesperada* | `job` |
| HA | `ha_quorum_ok`, `ha_fencing_armed` (+ `state`: Proxmox VE 9 reporta se o watchdog está `armed` ou em `standby` — em standby nada será fenced), `ha_master_active`, `ha_lrm_active` (`node`), `ha_resources_total`; por recurso `ha_resource_started`, `ha_resource_error` (1 no estado `error`, `fence` ou `recovery`), `ha_resource_state_info` (+ `state`) | `sid`, `node`, `vmid`, `type` |
| Gerenciador HA | `ha_node_online`, `ha_node_status_info` (+ `status`: `online`, `fence`, `gone`, `maintenance`), `ha_master_info` (o nó mantendo o CRM master), `ha_manager_age_seconds`; por nó `ha_lrm_mode_info` (+ `mode`, `state`), `ha_lrm_age_seconds`, `ha_lrm_stale` (1 quando o LRM não escreveu por mais de dois minutos) | `node` |
| Snapshots | `guest_snapshot_count`, `guest_snapshot_oldest_age_seconds`, `guest_snapshot_newest_age_seconds` (idades só quando contagem > 0); `guest_snapshot_guests_skipped` quando `max_snapshot_guests` é excedido | `vmid`, `name`, `node`, `type` |
| Replicação | `replication_job_enabled`, `replication_job_error` (mensagem de erro ou `fail_count` > 0), `replication_job_fail_count`, `replication_job_last_sync_age_seconds`, `replication_job_next_sync_seconds`, `replication_job_duration_seconds`; `replication_jobs_total` | `job`, `vmid`, `node` (origem), `to_node` |
| Ceph | `ceph_health` (0 OK, 1 WARN, 2 ERR, 3 desconhecido), `ceph_health_info` (+ `status`), `ceph_osds_total/up/in`, `ceph_bytes_total/used`, `ceph_used_percent`, `ceph_pgs_total`, `ceph_mons_total` | |
| Ceph OSDs | `ceph_osd_up`, `ceph_osd_in`, `ceph_osd_used_percent`, `ceph_osd_used_bytes`, `ceph_osd_total_bytes`, `ceph_osd_apply_latency_ms`, `ceph_osd_commit_latency_ms`, `ceph_osd_reweight`, `ceph_osd_crush_weight`; totais `ceph_osds_down`, `ceph_osds_out` | `osd`, `host`, `device_class` |
| Pools Ceph e CephFS | `ceph_pool_used_percent`, `ceph_pool_used_bytes`, `ceph_pool_size`, `ceph_pool_min_size`, `ceph_pool_pg_num`, `ceph_pool_pg_num_optimal` (o que o autoscaler escolheria), `ceph_pool_info` (+ `type`, `crush_rule`, `autoscale`), `ceph_pools_total`; `ceph_fs_info` (+ `metadata_pool`, `data_pool`), `ceph_fs_total` | `pool`; CephFS: `name` |
| Flags e silêncios Ceph | `ceph_flag` (1 enquanto a flag está definida), `ceph_flags_set`; `ceph_health_mute_info` (+ `sticky`), `ceph_health_mutes` | `flag`; silêncios: `code` |
| Discos físicos | `node_disk_size_bytes`, `node_disk_smart_failed` (1 quando a saúde SMART é `FAILED`; ausente quando o disco reporta `UNKNOWN`), `node_disk_health_info` (+ `health`, `used`: `LVM`, `ZFS`, `partitions`…), `node_disk_wearout_percent` (SSD e NVMe: 0 novo, 100 fim da vida nominal), `node_disk_temperature_celsius` (de `disks/smart`: atributo ATA 194/190, ou a linha de temperatura de um relatório de texto — `Temperature:` para NVMe, `Current Drive Temperature:` para SAS) | `node`, `disk` (`/dev/sda`, `/dev/nvme0n1`), `model`, `type` (`ssd`, `hdd`, `nvme`) |
| Pools ZFS | `node_zfs_pool_degraded` (1 a menos que `ONLINE`), `node_zfs_pool_health_info` (+ `health`), `node_zfs_pool_size/alloc/free_bytes`, `node_zfs_pool_used_percent`, `node_zfs_pool_fragmentation_percent` | `node`, `pool` |
| Atualizações | `node_updates_pending`, `node_updates_security_pending` (pacotes cuja origem, rótulo de repositório, suite, seção ou URL de changelog nomeia um arquivo de segurança, como o `bookworm-security` do Debian). Sem `Sys.Modify` essas duas não existem; `node_pve_packages_upgradable` ainda conta os pacotes Proxmox cuja versão candidata difere da instalada, e `node_reboot_required` (+ `running`, `installed`) diz que um kernel mais novo está instalado mas ainda não iniciado — ambos lidos de `apt/versions`, que só precisa de `Sys.Audit` | `node` |
| Mudanças de pacote | `node_packages_changed`: 0 normalmente; por uma hora depois de uma sondagem ver a versão instalada de um pacote Proxmox mudar (`apt/versions`), o número de pacotes que mudaram, com um rótulo `changes` como `pve-manager 8.2.4→8.2.7, proxmox-kernel-6.8 6.8.8-2→6.8.12-2` | `node`, `changes` |
| Assinatura e repositórios | `node_subscription_active`, `node_subscription_info` (+ `status`: `active`, `notfound`, `expired`…, `level`, `next_due`); `node_repository_errors` (arquivos de fontes ilegíveis), `node_repository_warnings` (próprios avisos do Proxmox: repositório enterprise sem assinatura, repositório de teste), `node_repository_enabled` (+ `repo`: `enterprise`, `no-subscription`, `test`, `ceph-*`) | `node` |
| Certificados | `node_certificate_expiry_days` (negativo uma vez expirado) | `node`, `filename`, `subject` |
| Fluxo RRD (desligado por padrão) | `node_rrd_<metric>`, `guest_rrd_<metric>`, `storage_rrd_<metric>` — toda medição que os próprios servidores de métrica do Proxmox recebem, sob seu próprio nome (veja `metrics_export` abaixo) | `node`; guests: `vmid`, `type`; storages: `node`, `storage` |
| Coleta | `up`, `scrape_errors`, `scrape_duration_seconds` | |

A idade do backup por máquina responde a falha mais cara de um homelab:
o backup que não roda há semanas, descoberto no dia da restauração. É
calculada a partir do log de tarefas `vzdump` (sobre
`backup_lookback_days`) e, se `scan_backup_storage` está ativado, dos
arquivos encontrados nos storages de backup.

Jobs agendados vêm de Datacenter → Backup: `backup_covered` é 0 para
todo guest que o próprio Proxmox lista como não coberto por nenhum job,
então uma máquina criada ontem aparece antes de seu primeiro backup
perdido em vez de depois.

Um job que cobre um guest não necessariamente escreve todos os seus
discos. Um volume marcado `backup=0` é pulado toda noite, o job ainda
reporta sucesso, e ninguém descobre até uma restauração.
`backup_job_guest_excluded_volumes` nomeia o job, o guest e o próprio
motivo do Proxmox para a exclusão — mas só quando a exclusão é uma
surpresa: um drive de CD-ROM, uma imagem cloudinit ou uma entrada que
não é um volume de forma alguma é excluída por design, contada em
`backup_job_volumes_excluded` e nunca reportada por guest. Sem essa
distinção toda máquina com uma unidade virtual geraria um alerta.

Os daemons de um nó valem a pena observar por um motivo:
`pvestatd` parado não quebra nada visivelmente — a interface web
continua mostrando os números do momento em que morreu, e tudo parece
normal. `node_core_services_down` conta apenas `pve-cluster`,
`pvedaemon`, `pveproxy`, `pvestatd`, `pve-firewall`, `corosync` e
`watchdog-mux`, e só os que o próprio nó declara como ativados, então um
nó autônomo sem corosync ou HA não conta nada faltando.

Uma interface conta como fora do ar quando está configurada para
iniciar no boot e não está ativa (ou não existe mais de forma alguma).
Uma ativada sob demanda, sem autostart, nunca é reportada: sua ausência
é normal. Uma bridge que falha ao voltar depois de um reinício corta
todo guest conectado a ela enquanto cada um desses guests roda
perfeitamente, e nada mais diz isso.

Um pool thin LVM que se enche torna todo guest armazenado nele somente
leitura de uma vez, e excluir arquivos dentro dos guests não devolve o
espaço. Seus metadados vivem em seu próprio volume, muito menor e
frequentemente saturam primeiro, com exatamente a mesma consequência —
por isso os dois níveis de ocupação são séries separadas e regras
separadas.

Mudanças de pacote são detectadas pelo próprio coletor: ele lembra as
versões instaladas da sondagem anterior (por monitor e nó, em
memória — um reinício do servidor aprende de novo sem reportar). Só um
pacote presente em ambas as sondagens com uma versão diferente conta;
um pacote recém-instalado ou removido não conta.

Uma falha parcial (um nó fora do ar, um storage lento) ainda é uma
sondagem bem sucedida: o que pôde ser lido é escrito e a falha é
contada em `scrape_errors`. Recursos simplesmente ausentes permanecem
silenciosos: nenhum Ceph, nenhum job de replicação, nenhum pool ZFS,
nenhum agente guest, ou nenhum direito de listar atualizações não
produzem série nem erro. O mesmo vale para tudo acima: um privilégio
faltando (403), um endpoint que um Proxmox mais antigo não tem (404 ou
501), um recurso não instalado (Ceph responde 500) não produzem erro e
nenhuma métrica de forma alguma — nunca um zero fingindo ser uma
medição.

Temperaturas de placa não estão disponíveis: a API do Proxmox VE não
expõe sensores, só a temperatura SMART própria dos discos. Instale o
agente DumbMonit no nó se você precisar de mais.

Regras integradas que se aplicam: Monitor unreachable, High CPU, Disk
almost full, Filesystem almost full (forecast), Backup too old, Unusual
CPU, VM or container stopped, VM or container CPU high, VM or container
memory high, VM or container disk almost full, HA resource in error,
Cluster lost quorum, Node offline, Storage almost full, Backup job
failed, Old snapshot, Replication failed, Ceph health error, Ceph
health warning, Updates pending, Security updates pending, Proxmox
packages changed, Proxmox disk wearing out, Proxmox disk SMART failure,
Proxmox ZFS pool degraded, Node certificate expiring, Thin pool almost
full, Thin pool metadata almost full, Proxmox service down, Node
network interface down, Ceph OSD down, Ceph OSD nearly full, Ceph pool
almost full, Ceph noout flag left on, HA manager not reporting, Backup
job excludes a disk, Guest locked.

### O fluxo RRD completo {#the-full-rrd-stream}

`/cluster/metrics/export` é o endpoint que os próprios servidores de
métrica do Proxmox (InfluxDB, Graphite) consomem: tudo que `pvestatd`
mede, para todo o cluster, em uma chamada. Duas coisas fazem valer a
pena tê-lo.

* Ele carrega medições que nada mais expõe — em particular a
  informação de pressão (PSI) por nó: quanto tempo tarefas gastam
  esperando pela CPU, o disco ou memória. É esse o número que explica
  "tudo está lento" quando CPU e memória parecem bem.
* É lido com `history=1` a partir do último ponto já visto, então todo
  ponto entre duas sondagens é ingerido em seu próprio timestamp. Um
  monitor sondado a cada cinco minutos recebe seus cinco pontos de
  um minuto em vez de um único valor atual.

Está **desligado por padrão**, e essa é uma escolha deliberada. Essas
séries parcialmente repetem as que o resto da coleta já produz sob
nomes melhores, então ativar `metrics_export` adiciona um grande número
de séries para armazenar e filtrar por um detalhe que a maioria das
instalações nunca olha. Os nomes são publicados exatamente como o
Proxmox os envia (`node_rrd_pressure_cpu_some_avg10`, `guest_rrd_netin`…)
em vez de traduzidos: o Proxmox adiciona novos em toda versão, e uma
tabela de tradução escrita aqui estaria errada na próxima. Precisa de
`Sys.Audit` em `/` — que o papel `DumbMonit` já tem quando concedido em
`/` como abaixo.

## O que preparar no Proxmox {#what-to-prepare-in-proxmox}

Os passos abaixo são os que o aviso ao lado do formulário mostra. O
princípio: um usuário reservado para monitoramento, um papel que só
pode ler, um token — nunca a conta com a qual você entra.

1. Abra um shell em qualquer nó (na interface web: selecione o nó,
   depois Shell; ou SSH) e crie um usuário reservado para
   monitoramento. Não precisa de senha: o token é o que entra.

    ```
    pveum user add dumbmonit@pve --comment "DumbMonit monitoring"
    ```

2. Crie um papel apenas com os privilégios que o coletor usa:
   Sys.Audit (nós, cluster, HA, discos e SMART, ZFS, certificados,
   versões de pacote, assinatura), Datastore.Audit (storages e
   arquivos de backup), VM.Audit (VMs, contêineres, snapshots) e
   VM.GuestAgent.Audit (o que o agente QEMU guest lê dentro de uma VM:
   uso de disco, sistema operacional, endereços IP). Nenhum deles pode
   alterar nada.

    ```
    pveum role add DumbMonit --privs "Datastore.Audit Sys.Audit VM.Audit VM.GuestAgent.Audit"
    ```

3. Dê ao usuário esse papel em todo o cluster.

    ```
    pveum aclmod / -user dumbmonit@pve -role DumbMonit
    ```

4. Crie o token de API do usuário. A separação de privilégios está
   desligada, então o token simplesmente herda os direitos do usuário.

    ```
    pveum user token add dumbmonit@pve monitor --privsep 0
    ```

5. O comando imprime uma tabela com full-tokenid e value. Copie
   full-tokenid para o campo Token ID do DumbMonit e value (o UUID)
   para seu campo Secret. O secret é mostrado uma vez: se for perdido,
   remova o token e crie um novo.

    ```
    dumbmonit@pve!monitor
    ```

6. Opcional, para contar atualizações pendentes e correções de
   segurança pendentes: o Proxmox protege essa lista com Sys.Modify.
   Conceda-o apenas em /nodes; sem ele o coletor pula a lista
   silenciosamente.

    ```
    pveum role add DumbMonitUpdates --privs Sys.Modify
    pveum aclmod /nodes -user dumbmonit@pve -role DumbMonitUpdates
    ```

7. Em um Proxmox mais antigo, VM.GuestAgent.Audit pode ainda não
   existir e o comando acima é recusado com privilégio inválido: as
   chamadas de agente guest são então cobertas por VM.Monitor, que o
   Proxmox VE 9 removeu por sua vez. Use o que sua versão conhece.

    ```
    pveum role add DumbMonit --privs "Datastore.Audit Sys.Audit VM.Audit VM.Monitor"
    ```

8. Prefere a interface web? Os mesmos passos vivem sob Datacenter →
   Permissions: Users, Roles, Add → User Permission, depois API
   Tokens com "Privilege Separation" desmarcado.
9. No DumbMonit, informe o endereço de qualquer nó (porta 8006 por
   padrão).

!!! warning
    Não reutilize a conta com a qual você entra: um token exposto então
    controlaria todo o cluster. O papel DumbMonit acima só pode ler. O
    Proxmox também usa um certificado autoassinado por padrão: se a
    conexão for recusada por esse motivo, marque "Aceitar um
    certificado não verificável" nas opções.

Documentação do fornecedor: [Proxmox VE user management](https://pve.proxmox.com/wiki/User_Management).

## Credenciais {#credentials}

| Credencial | Campos |
|---|---|
| Token de API (recomendado) | **Token ID**: usuário, realm e nome do token como o Proxmox os mostra, `dumbmonit@pve!monitor`. **Secret**: o UUID mostrado uma vez quando o token foi criado. Sem expiração, nenhuma sessão aberta no hipervisor. |
| Nome de usuário / senha | `user@realm` e a senha. Um ticket é obtido, armazenado em cache e renovado dez minutos antes de sua expiração de duas horas. |

A API aceita o token em duas partes (`token_id` + `secret`) ou, como
versões mais antigas faziam, como uma string
`token` = `user@realm!name=secret`; ambos são armazenados da mesma
forma. Um token completo colado em Token ID é separado pelo formulário.

O papel `DumbMonit` acima carrega o que `PVEAuditor` concede para os
caminhos que o coletor lê, mais o privilégio de agente guest: o coletor
sempre só faz `GET`. Por endpoint: `Sys.Audit` para `/version`,
`/cluster/*`, `/nodes/{node}/status`, `services`, `version`, `network`,
`netstat`, `tasks`, `replication`, `certificates/info`, `disks/list`,
`disks/smart`, `disks/zfs`, `apt/versions`, `apt/repositories` e
`subscription`; `Datastore.Audit` para `storage` e
`storage/{id}/content`; `VM.Audit` para `qemu`, `lxc`, `snapshot`,
`status/current` e as `interfaces` de um contêiner; `VM.GuestAgent.Audit`
— ou `VM.Monitor` antes do Proxmox VE 9 — para `agent/get-fsinfo`.

Dois caminhos pedem um pouco mais que o resto, e ambos são cobertos
pelo papel concedido no passo 3 — em `/`, não em `/nodes`:

* `Sys.Audit` **em `/`** para o inventário LVM (`disks/lvm`,
  `disks/lvmthin`, `disks/directory`) e para `/cluster/metrics/export`,
  o fluxo RRD opcional. Concedido só em `/nodes`, as séries de thin
  pool e as séries `*_rrd_*` ficam silenciosamente ausentes.
* `VM.GuestAgent.Audit` para as duas chamadas de agente guest que leem
  o sistema operacional e endereços de uma VM (`agent/get-osinfo`,
  `agent/network-get-interfaces`). O Proxmox VE 7 não tinha esse
  privilégio e usava `VM.Monitor`; o Proxmox VE 9 removeu `VM.Monitor`.
  Sem o que sua versão conhece, `guest_os_info` e `guest_ip_info`
  simplesmente não são publicados para VMs; contêineres, que não
  precisam de agente, mantêm seus endereços.

A única coisa que o papel não cobre de forma alguma é a lista de
atualizações pendentes (`apt/update`), que o Proxmox protege com
`Sys.Modify` em `/nodes` — opcional, como o passo 6 explica; sem ele o
coletor pula `node_updates_pending` e `node_updates_security_pending`
silenciosamente.

(Use `-token 'dumbmonit@pve!monitor'` em vez de `-user` nos comandos
`aclmod` para um token criado com "Privilege Separation" marcado.)

Endereço: `10.0.0.10`, `pve.lan`, `pve.lan:8006`, `[fd00::1]` ou uma
URL completa `https://pve.example.net`. O esquema `https` e a porta
8006 são adicionados se faltando.

## Opções {#options}

| Chave | Rótulo | Padrão | Ajuda |
|---|---|---|---|
| `port` | Porta da API | `8006` | Usada se o endereço não der uma porta. |
| `insecure_tls` | Aceitar um certificado não verificável | `false` | O Proxmox vem com um certificado autoassinado por padrão: ative isso se a conexão for recusada por esse motivo. |
| `request_timeout_seconds` | Tempo limite por requisição (segundos) | `10` | Tempo permitido para cada chamada de API, de 1 a 120. |
| `backup_lookback_days` | Retrospecto de backup (dias) | `31` | Tarefas de backup mais antigas não são examinadas, de 1 a 3650. |
| `scan_backup_storage` | Inventariar arquivos de backup | `true` | Varre os storages de backup para datar o último backup de cada máquina. Desative se o storage for lento para responder. |
| `nodes` | Nós monitorados | *(vazio)* | Nomes dos nós a monitorar, separados por vírgulas. Vazio: todo nó no cluster. |
| `cluster_resources` | Ler o inventário do cluster | `true` | Uma chamada a `/cluster/resources` lista todo nó, guest, storage e pool. Mantenha ativado: é a única fonte que ainda vê os guests de um nó que parou de responder, e economiza duas chamadas por nó. Desligado, o coletor percorre `/nodes/{node}/qemu` e `/nodes/{node}/lxc` em vez disso, e perde os pools e bloqueios. |
| `services` | Observar serviços do nó | `true` | `/nodes/{node}/services` e `version`: o estado dos daemons Proxmox de cada nó e a versão instalada nele. Um `pvestatd` parado deixa todo o cluster mostrando números congelados. |
| `network` | Observar a rede do nó | `true` | `/nodes/{node}/network` e `netstat`: bridges, bonds e VLANs com seu estado de link, mais os contadores de tráfego de cada placa de rede de guest. |
| `lvm` | Observar LVM e pools thin | `true` | `disks/lvm`, `disks/lvmthin` e `disks/directory`: grupos de volume, pools thin (ocupação de dados *e* metadados) e montagens de diretório gerenciadas pelo PVE. Precisa de `Sys.Audit` em `/`; silencioso caso contrário. |
| `ha` | Alta disponibilidade | `true` | Lê `/cluster/ha/status/current` e `manager_status`: quórum, CRM master, a própria visão do gerenciador de cada nó, todo LRM com a idade de seu último relatório, e todo recurso HA. |
| `backup_jobs` | Jobs de backup agendados | `true` | Lê os jobs de Datacenter → Backup e os guests que nenhum job cobre. |
| `scan_snapshots` | Inventariar snapshots | `true` | Uma chamada por guest (quatro em paralelo por nó). Desative em clusters muito grandes. |
| `max_snapshot_guests` | Limite de inventário de snapshot | `200` | Guests além desse número são pulados a cada sondagem e contados em `guest_snapshot_guests_skipped`, de 1 a 10000. |
| `replication` | Jobs de replicação | `true` | Status de replicação de armazenamento por nó. Silencioso quando o nó não tem job. |
| `ceph` | Saúde do Ceph | `true` | Lê `/cluster/ceph/status`. Silencioso quando o Ceph não está instalado. |
| `ceph_detail` | Observar o Ceph em detalhe | `true` | Estado, uso e latência por OSD, uso por pool, CephFS, flags de OSD e verificações de saúde silenciadas. Três chamadas em um nó mais duas em todo o cluster, qualquer que seja o tamanho do cluster. Precisa de `ceph` ativado; silencioso sem Ceph. |
| `backup_volumes` | Verificar o que jobs de backup incluem | `true` | Para cada job agendado, quais guests cobre e quais de seus discos realmente escreve. Pega um job que tem sucesso toda noite enquanto pula um disco de dados. Uma chamada por job. |
| `guest_os` | Ler SO e endereços do guest | `true` | Sistema operacional e endereços IP vistos de dentro de cada guest em execução (agente QEMU guest para VMs, o namespace de contêiner para contêineres). Atualizado uma vez por hora por guest, não a cada sondagem. Precisa de `VM.GuestAgent.Audit` no Proxmox VE 8. |
| `metrics_export` | Ingerir o fluxo de métrica RRD completo | `false` | Lê `/cluster/metrics/export`, o fluxo que os próprios servidores de métrica do Proxmox consomem, incluindo pressão por nó (PSI), e todo ponto desde a última sondagem em vez de só o atual. Desligado por padrão: as séries `*_rrd_*` que adiciona parcialmente repetem as já coletadas. Precisa de `Sys.Audit` em `/`. |
| `updates` | Atualizações pendentes | `true` | Conta os pacotes que `apt/update` lista. Precisa de `Sys.Modify` em `/nodes` (veja acima); silencioso caso contrário. |
| `certificates` | Expiração de certificado | `true` | Dias restantes em cada certificado de nó (`pve-ssl.pem`, `pveproxy-ssl.pem`, `pve-root-ca.pem`). |
| `guest_agent` | Perguntar ao agente QEMU guest | `true` | Para cada VM em execução: `status/current` (balloon, flag de agente) e, quando o agente está ativado, `agent/get-fsinfo` (precisa de `VM.GuestAgent.Audit`). Quatro chamadas em paralelo por nó. Silencioso quando o agente está ausente. |
| `disks` | Observar discos físicos | `true` | `disks/list` mais uma chamada `disks/smart` por disco: saúde, desgaste, temperatura. |
| `zfs` | Observar pools ZFS | `true` | `disks/zfs`: saúde, capacidade, fragmentação. Silencioso sem ZFS. |
| `packages` | Detectar mudanças de pacote | `true` | `apt/versions` comparado com a sondagem anterior; uma mudança é reportada por uma hora em `node_packages_changed`. |
| `subscription` | Observar assinatura e repositórios | `true` | `subscription` e `apt/repositories`: status, avisos, repositórios padrão ativados. |

## Regras de alerta integradas

Estas regras vêm com o DumbMonit e ficam ativas desde o início. Cada uma pode ser editada ou desativada em Alertas; a [referência das regras integradas](../alerting/rules.md#proxmox-ve) lista todas as regras de todas as integrações.

| Regra | O que é | Limite padrão | Retenção | Severidade | Lembrete |
|---|---|---|---|---|---|
| Backup too old | Nenhum backup Proxmox VE bem sucedido por mais de sete dias (`dumbmonit_proxmox_backup_last_age_seconds`). | > 7 d | 1 h | Aviso (`warning`) | 24 h |
| VM or container stopped | O guest estava rodando nas últimas duas horas e está parado há cinco minutos (`1 - dumbmonit_proxmox_guest_running`, combinado com `max_over_time(…[2h]) == 1`). | > 0 | 5 min | Aviso (`warning`) | 6 h |
| VM or container CPU high | Um guest usou mais de 90% de seus núcleos alocados por quinze minutos (`dumbmonit_proxmox_guest_cpu_percent`). A notificação nomeia o guest, `nextcloud (202)`. | > 90 % | 15 min | Aviso (`warning`) | 6 h |
| VM or container memory high | Um guest usou mais de 95% de sua memória por dez minutos (`dumbmonit_proxmox_guest_memory_percent`). | > 95 % | 10 min | Aviso (`warning`) | 6 h |
| VM or container disk almost full | O disco raiz de um guest está mais de 90% cheio (`dumbmonit_proxmox_guest_disk_used_percent`): contêineres sempre, VMs só quando o agente QEMU guest reporta uso. | > 90 % | 15 min | Aviso (`warning`) | 24 h |
| Guest locked | Uma VM ou contêiner está bloqueado há seis horas (`dumbmonit_proxmox_guest_locked`, rótulo `lock`): um backup ou migração interrompido deixou o bloqueio e nenhuma operação pode rodar no guest, nem mesmo o próximo backup. | > 0 | 6 h | Aviso (`warning`) | 24 h |
| Old snapshot | O snapshot mais antigo de um guest tem mais de trinta dias (`dumbmonit_proxmox_guest_snapshot_oldest_age_seconds`). | > 30 d | 1 h | Info (`info`) | 7 d |
| HA resource in error | Um recurso HA do Proxmox está em erro ou fenced (`dumbmonit_proxmox_ha_resource_error`). | > 0 | 2 min | Alerta (`critical`) | 1 h |
| HA manager not reporting | O gerenciador de recursos local HA de um nó parou de escrever seu estado (`dumbmonit_proxmox_ha_lrm_stale`): os guests HA daquele nó não serão reiniciados nem realocados. | > 0 | 5 min | Alerta (`critical`) | 1 h |
| Cluster lost quorum | O cluster Proxmox não tem quórum (`1 - dumbmonit_proxmox_cluster_quorate`): guests não podem mais ser iniciados. | > 0 | 1 min | Alerta (`critical`) | 30 min |
| Proxmox node offline | Um nó do cluster é reportado offline (`1 - dumbmonit_proxmox_node_up`). | > 0 | 2 min | Alerta (`critical`) | 30 min |
| Proxmox service down | Um daemon central do Proxmox não está rodando em um nó (`dumbmonit_proxmox_node_core_services_down`: `pve-cluster`, `pvedaemon`, `pveproxy`, `pvestatd`, `pve-firewall`, `corosync`, `watchdog-mux`, cada um contado só onde o nó o ativa). | > 0 | 5 min | Alerta (`critical`) | 1 h |
| Node network interface down | Uma interface configurada para iniciar no boot não está ativa em um nó (`dumbmonit_proxmox_node_interface_offline`). Uma interface ativada sob demanda nunca é reportada. | > 0 | 5 min | Aviso (`warning`) | 6 h |
| Proxmox storage almost full | Um storage Proxmox está mais de 85% cheio (`dumbmonit_proxmox_storage_used_percent`); "Disk almost full" assume em 90%. | > 85 % | 15 min | Aviso (`warning`) | 24 h |
| Thin pool almost full | Um pool thin LVM está mais de 90% cheio: todo guest armazenado nele fica somente leitura quando se enche (`dumbmonit_proxmox_node_thinpool_used_percent`); limpa abaixo de 88%. | > 90 % | 15 min | Alerta (`critical`) | 6 h |
| Thin pool metadata almost full | O volume de metadados de um pool thin LVM está mais de 80% cheio (`dumbmonit_proxmox_node_thinpool_metadata_used_percent`). É muito menor que o volume de dados e frequentemente satura primeiro, com a mesma consequência. | > 80 % | 15 min | Alerta (`critical`) | 6 h |
| Proxmox disk wearing out | Um SSD ou NVMe de um nó usou mais de 90% de sua vida nominal (`dumbmonit_proxmox_node_disk_wearout_percent`). A notificação lê `pve1 · /dev/nvme0n1`. | > 90 % | 1 h | Aviso (`warning`) | 7 d |
| Proxmox disk SMART failure | Um disco de um nó reporta saúde SMART `FAILED` (`dumbmonit_proxmox_node_disk_smart_failed`). | > 0 | 5 min | Alerta (`critical`) | 24 h |
| Proxmox ZFS pool degraded | Um pool ZFS de um nó não está `ONLINE` (`dumbmonit_proxmox_node_zfs_pool_degraded`). A notificação lê `pve2 · tank`. | > 0 | 5 min | Alerta (`critical`) | 6 h |
| Backup job failed | O último job vzdump em um nó, ou um job de backup agendado, falhou (`1 - dumbmonit_proxmox_backup_job_last_ok`). | > 0 | 10 min | Alerta (`critical`) | 24 h |
| Backup job excludes a disk | Um job de backup agendado pula um disco de um dos guests que cobre (`dumbmonit_proxmox_backup_job_guest_excluded_volumes`): o job tem sucesso toda noite com esses dados faltando. Drives de CD-ROM e imagens cloudinit são excluídas por design e nunca contadas. | > 0 | 1 h | Aviso (`warning`) | 7 d |
| Replication failed | Um job de replicação reporta um erro ou uma contagem de falha diferente de zero (`dumbmonit_proxmox_replication_job_error`): a cópia em standby está desatualizada. | > 0 | 10 min | Alerta (`critical`) | 6 h |
| Ceph health error | O Ceph reporta `HEALTH_ERR` (`dumbmonit_proxmox_ceph_health`, 0 OK, 1 WARN, 2 ERR). | ≥ 2 | 2 min | Alerta (`critical`) | 30 min |
| Ceph health warning | O Ceph reporta `HEALTH_WARN` por mais de quinze minutos (`dumbmonit_proxmox_ceph_health == 1`). | > 0 | 15 min | Aviso (`warning`) | 6 h |
| Ceph OSD down | Um OSD Ceph está fora do ar há cinco minutos (`1 - dumbmonit_proxmox_ceph_osd_up`). A notificação nomeia o OSD e seu host. | > 0 | 5 min | Alerta (`critical`) | 1 h |
| Ceph OSD nearly full | Um único OSD Ceph está mais de 85% cheio (`dumbmonit_proxmox_ceph_osd_used_percent`): escritas param quando alcança seu limite, qualquer que seja a média do cluster; limpa abaixo de 82%. | > 85 % | 15 min | Aviso (`warning`) | 24 h |
| Ceph pool almost full | Um pool Ceph está mais de 85% cheio (`dumbmonit_proxmox_ceph_pool_used_percent`); limpa abaixo de 82%. | > 85 % | 15 min | Aviso (`warning`) | 24 h |
| Ceph noout flag left on | A flag `noout` do Ceph está definida há duas horas (`dumbmonit_proxmox_ceph_flag{flag="noout"}`): o rebalanceamento está desativado, e o cluster lê saudável enquanto sua redundância diminui. | > 0 | 2 h | Info (`info`) | 24 h |
| Proxmox updates pending | Mais de vinte atualizações de pacote estão pendentes em um nó (`dumbmonit_proxmox_node_updates_pending`). | > 20 | 1 h | Info (`info`) | 7 d |
| Proxmox security updates pending | Ao menos uma atualização pendente vem de um arquivo de segurança (`dumbmonit_proxmox_node_updates_security_pending`). | > 0 | 1 h | Aviso (`warning`) | 7 d |
| Proxmox packages changed | A versão instalada de um pacote Proxmox mudou entre duas sondagens — alguém executou `apt upgrade` (`dumbmonit_proxmox_node_packages_changed`, publicado por uma hora com o detalhe em seu rótulo `changes`: `pve1 · pve-manager 8.2.4→8.2.7`). Resolve por conta própria uma hora depois. | > 0 | 1 min | Info (`info`) | 24 h |
| Node certificate expiring | Um certificado de nó Proxmox expira em menos de quatorze dias (`dumbmonit_proxmox_node_certificate_expiry_days`). | < 14 d | 1 h | Aviso (`warning`) | 24 h |
| Proxmox storage inactive | Um armazenamento ativado não está ativo em um nó (compartilhamento inacessível, disco não montado, servidor de backup fora do ar): os backups para ele e os discos que ele hospeda falham (`dumbmonit_proxmox_storage_active`). | < 1 | 10 min | Aviso (`warning`) | 6 h |
| Proxmox node needs a reboot | Um kernel mais recente está instalado em um nó há três dias sem reinicialização: o nó continua rodando o antigo (`dumbmonit_proxmox_node_reboot_required`). | > 0 | 3 d | Info (`info`) | 7 d |
| Proxmox subscription expired | A assinatura de um nó está expirada, inválida ou suspensa: o repositório enterprise para de servir atualizações (`dumbmonit_proxmox_node_subscription_info`). | > 0 | 1 h | Aviso (`warning`) | 7 d |
| Proxmox replication overdue | Um job de replicação de armazenamento está mais de seis horas atrasado em relação ao seu agendamento: a cópia no outro nó está envelhecendo (`dumbmonit_proxmox_replication_job_next_sync_seconds`). | > 6 h | 15 min | Aviso (`warning`) | 12 h |
| Proxmox backup job not running | Um job de backup ativado não roda há mais de oito dias (`dumbmonit_proxmox_backup_job_last_run_age_seconds`). | > 8 d | 1 h | Aviso (`warning`) | 24 h |
| Proxmox guest not in any backup job | Uma VM ou um contêiner não está em nenhum job de backup há um dia (`dumbmonit_proxmox_backup_covered`). | < 1 | 24 h | Info (`info`) | 7 d |
| Proxmox tasks failed | Tarefas falharam em um nó nas últimas seis horas (inicialização de convidado, migração, restauração…). Os backups têm sua própria regra (`dumbmonit_proxmox_node_tasks_failed`). | > 0 | 5 min | Info (`info`) | 24 h |

## Limites conhecidos

- As temperaturas da placa-mãe não estão disponíveis: a API do Proxmox VE só dá a temperatura SMART dos discos. Instale o agente DumbMonit no nó para os outros sensores.
- O uso de disco dentro de uma VM precisa do agente convidado do QEMU. Sem ele, a VM tem um tamanho e nenhum uso.
- O sistema operacional e os endereços de um convidado são solicitados no máximo uma vez por hora.
- Os snapshots são inventariados para no máximo 200 convidados por sondagem (opção `max_snapshot_guests`); os demais são contados como ignorados.
- Um privilégio ausente, um Proxmox VE mais antigo ou um recurso não instalado nunca gera um erro: as séries correspondentes ficam ausentes, e a página do monitor nomeia o privilégio ausente.

## Erros comuns {#common-errors}

| Sintoma | Causa provável |
|---|---|
| Erro de certificado mostrado no monitor | Certificado autoassinado. Ou instale um certificado confiável (ACME está embutido no Proxmox) ou ative `insecure_tls`. A opção nunca é ativada implicitamente. |
| Erro de autenticação | Secret colado em Token ID (ou o contrário), "Privilege Separation" deixado marcado (o token então não tem direitos), ou o papel `DumbMonit` não aplicado em `/`. |
| Idade de backup faltando para um guest | Nenhuma tarefa `vzdump` na janela de retrospecto e nenhum arquivo encontrado nos storages; ou `scan_backup_storage` desativado. |
| Sondagens lentas | Um storage de backup que leva tempo para listar, ou muitos guests para inventariar por snapshots. Desative `scan_backup_storage` ou `scan_snapshots`, diminua `max_snapshot_guests`, ou aumente `request_timeout_seconds`. |
| `node_updates_pending` faltando | O usuário ou token não tem `Sys.Modify` em `/nodes`. Conceda o papel `DumbMonitUpdates` do passo 6, ou ignore: `node_pve_packages_upgradable` e `node_reboot_required` cobrem os pacotes Proxmox sem isso. |
| Nenhuma porcentagem de memória para uma VM | O Proxmox VE 9 reporta, no inventário do cluster, a memória que a VM toma **no host** — overhead incluído, então regularmente acima de sua alocação. Uma porcentagem calculada a partir disso ficaria acima de 100% para sempre. O número honesto vem do driver balloon dentro do guest; uma VM com balloon desativado portanto mostra bytes e nenhuma porcentagem. `guest_memory_host_bytes` é o número do lado do host. |
| Nenhuma capacidade para o storage de backup | Um datastore Proxmox Backup Server montado no PVE não reporta tamanho: o PVE não o lê. A própria página de monitor do datastore tem isso. |
| Uso de disco vazio ("apenas tamanho") para uma VM | A VM não tem agente QEMU guest (não ativado em suas opções, ou `qemu-guest-agent` não instalado dentro), ou o token não tem `VM.GuestAgent.Audit`. `guest_agent_enabled` e `guest_agent_running` dizem qual. Contêineres nunca precisam disso. |
| Nenhuma série `node_disk_*` | O token não tem `Sys.Audit` em `/` (o papel `DumbMonit` o concede), ou o nó roda detrás de um controlador RAID que esconde o SMART: `disks/list` então reporta `UNKNOWN` e nenhum `node_disk_smart_failed` é publicado. |
| `node_packages_changed` dispara depois de um reinício de servidor | Não dispara: a primeira sondagem depois de um reinício só aprende as versões. Uma mudança é reportada só entre duas sondagens consecutivas do mesmo processo de servidor. |
| `backup_job_last_ok{job=…}` faltando enquanto `backup_job_last_ok{node=…}` existe | As tarefas vzdump não carregam o id do job (Proxmox VE mais antigo que 7.2, ou jobs executados manualmente). O status de execução por nó ainda está lá. |
| Nenhuma métrica de Ceph ou replicação | Nada a reportar: o Ceph não está instalado, ou nenhum job de replicação está configurado. Não é um erro. |
| Nenhum pool thin ou grupo de volume em um cartão de nó | A opção `lvm` está desligada, o nó não tem LVM, ou o usuário tem `Sys.Audit` apenas em `/nodes` — esse inventário o pede em `/`. |
| A seção Ceph está faltando enquanto o cluster tem Ceph | `ceph` ou `ceph_detail` está desligado, ou todo nó respondeu às chamadas de detalhe com um erro. `available: false` em `/api/targets/{id}/proxmox/ceph` significa "nenhum Ceph aqui", não "ainda não coletado". |
| Nenhum sistema operacional ou endereço IP para uma VM | O agente QEMU guest não está ativado ou instalado naquela VM, ou o token não tem `VM.GuestAgent.Audit` no Proxmox VE 8 (passo 7). Contêineres não precisam de agente. Um guest que acabou de iniciar espera até uma hora: os fatos são atualizados a cada hora, não a cada sondagem. |
| Um guest mantém um badge de bloqueio | Um backup, migração ou snapshot interrompido o deixou. `qm unlock <vmid>` para uma VM, `pct unlock <vmid>` para um contêiner — depois de verificar que o job realmente terminou. |
| `pve_backup_excludes_disk` dispara em uma máquina com um CD-ROM | Não dispara: drives, imagens cloudinit e entradas que não são volumes são excluídas por design e nunca reportadas por guest. O que disparou é um disco real carregando `backup=0`. |
| Nenhuma série `*_rrd_*` | `metrics_export` está desligado (é por padrão), ou o usuário não tem `Sys.Audit` em `/`. |
