# VMware vSphere {#vmware-vsphere}

Um vCenter Server e os anfitriões que gere, ou um anfitrião ESXi autónomo:
anfitriões que deixam de responder, ficam em modo de manutenção ou reportam
saúde vermelha, máquinas virtuais e as suas VMware Tools, datastores a encher
ou a ficarem inacessíveis, e os alarmes que o próprio vSphere levantou.

O DumbMonit lê o vSphere com um utilizador dedicado que tem o papel incorporado
**Read-only**. Nunca liga nem desliga uma VM, nunca entra em modo de manutenção
e nunca reconhece um alarme.

A falha que vale a pena vigiar: **um datastore que enche**. Quando um datastore
com thin provisioning chega a 100%, todas as VM que precisam de escrever nele
são pausadas de uma vez. Os alarmes que o vCenter levanta também o dizem, mas
só a quem tem o cliente aberto.

## Porquê a Web Services API {#why-the-web-services-api}

O DumbMonit fala com a vSphere Web Services API (SOAP, `/sdk`), não com a API
REST (`/api/vcenter/*`): um anfitrião ESXi autónomo não tem API REST, e num
vCenter a API REST não indica o modo de manutenção, a saúde global
(`overallStatus`), o estado das VMware Tools nem os alarmes disparados. Uma
vista de todo o inventário é lida numa única passagem, página a página, e
libertada no fim.

A sessão é mantida de uma sonda para a seguinte e só reaberta quando expira: o
vCenter não regista um novo início de sessão a cada minuto.

## O que monitoriza {#what-it-watches}

Todas as métricas têm o prefixo `dumbmonit_vsphere_`. Os valores de saúde
seguem o `overallStatus` do vSphere: 0 verde, 1 amarelo, 2 vermelho, 3 cinzento
(desconhecido).

| Métrica | O quê | Etiquetas |
|---|---|---|
| `info` | valor 1 | `product` (`vcenter` ou `esxi`), `version`, `build` |
| `hosts` | anfitriões por estado de ligação: `connected`, `disconnected`, `notResponding` | `state` |
| `host_connection_state` | 0 ligado, 1 desligado por um administrador, 2 sem resposta | `esxi` |
| `host_power_state` | 0 ligado, 1 em espera, 2 desligado, 3 desconhecido | `esxi` |
| `host_maintenance` | 1 em modo de manutenção | `esxi` |
| `host_status` | saúde global | `esxi` |
| `host_cpu_percent`, `host_memory_percent`, `host_memory_bytes`, `host_uptime_seconds` | apenas anfitriões ligados | `esxi` |
| `vms` | VM por estado de energia: `poweredOn`, `poweredOff`, `suspended` (modelos excluídos) | `state` |
| `templates` | modelos de VM | |
| `vm_power_state` | 0 ligada, 1 desligada, 2 suspensa; no máximo 1000 VM | `vm` |
| `vm_status` | saúde global | `vm` |
| `vm_tools_status` | apenas VM ligadas: 0 atualizado, 1 versão antiga, 2 instalado mas não a correr, 3 não instalado | `vm` |
| `vm_snapshots` | número de snapshots da VM; apenas para uma VM que tem pelo menos um | `vm` |
| `vm_snapshot_oldest_age_seconds` | idade do snapshot mais antigo da VM; apenas para uma VM que tem pelo menos um | `vm` |
| `vm_consolidation_needed` | 1 quando o vSphere indica que os discos da VM precisam de consolidação | `vm` |
| `datastores` | datastores | |
| `datastore_capacity_bytes`, `datastore_free_bytes`, `datastore_used_percent` | | `datastore` |
| `datastore_accessible` | 1 acessível, 0 inacessível | `datastore` |
| `datastore_status` | saúde global | `datastore` |
| `alarms` | alarmes disparados não reconhecidos, por cor: `red`, `yellow` | `status` |
| `alarms_acknowledged` | alarmes disparados já reconhecidos | |
| `alarm` | uma série por alarme não reconhecido, valor = o seu código de saúde, no máximo 64 | `alarm` (o seu nome), `entity`, `entity_type`, `status` |
| `scrape_errors` | chamadas que falharam na última sonda | |

Os snapshots são lidos da árvore de snapshots de cada VM, na mesma passagem que o resto do inventário. O DumbMonit guarda o seu número e a data do mais antigo, nunca os seus nomes. Uma VM sem snapshot não tem nenhuma série de snapshot.

As [regras incorporadas](../alerting/rules.md#vmware-vsphere) que se aplicam:

| Regra | O quê | Limiar predefinido | Retenção | Severidade | Lembrete |
|---|---|---|---|---|---|
| vSphere host not responding | Um anfitrião vSphere não está a responder ao vCenter (`dumbmonit_vsphere_host_connection_state`: 2 sem resposta). Um anfitrião desligado de propósito (1) não dispara. | ≥ 2 | 3 min | Warning (`critical`) | 1 h |
| vSphere host health red | O vSphere reporta a saúde geral de um anfitrião como vermelha (`dumbmonit_vsphere_host_status == 2`). Cinza significa desconhecido e não dispara. | > 0 | 5 min | Warning (`critical`) | 1 h |
| vSphere host left in maintenance | Um anfitrião vSphere está em modo de manutenção há um dia: as suas VMs foram movidas e pode ter sido esquecido lá (`dumbmonit_vsphere_host_maintenance`). | > 0 | 24 h | Info (`info`) | 24 h |
| vSphere datastore almost full | Um datastore vSphere está 90% cheio ou mais (`dumbmonit_vsphere_datastore_used_percent`); limpa-se abaixo de 85%. | ≥ 90 % | 15 min | Advisory (`warning`), agrava depois de 24 h | 6 h |
| vSphere datastore inaccessible | Um datastore vSphere está inacessível: as VMs guardadas nele não podem correr (`dumbmonit_vsphere_datastore_accessible`). | < 1 | 5 min | Warning (`critical`) | 1 h |
| vSphere VM tools not running | As VMware Tools estão instaladas mas não em execução numa VM ligada: o convidado pode estar pendurado (`dumbmonit_vsphere_vm_tools_status == 2`). VMs sem Tools não disparam. | > 0 | 30 min | Info (`info`) | 24 h |
| vSphere red alarm | O vSphere levantou um alarme vermelho que ninguém confirmou (`dumbmonit_vsphere_alarm{status="red"}`, uma série por alarme, nomeada pelo alarme e pelo objeto). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| vSphere VM snapshot too old | O snapshot mais antigo de uma VM tem mais de três dias: os snapshots crescem, tornam a VM mais lenta e podem encher o datastore. A VMware recomenda não manter nenhum além de 72 horas (`dumbmonit_vsphere_vm_snapshot_oldest_age_seconds`). | > 3 d | 1 h | Advisory (`warning`) | 24 h |
| vSphere VM disks need consolidation | O vSphere indica que os discos de uma VM precisam de consolidação: a remoção de um snapshot deixou ficheiros delta, e eles continuam a crescer (`dumbmonit_vsphere_vm_consolidation_needed`). | > 0 | 1 h | Advisory (`warning`) | 24 h |

## A página do monitor {#the-device-page}

O painel acima dos gráficos lê o que a sonda guardou; abrir a página nunca
consulta o vSphere. Diz primeiro o que está errado (anfitriões sem resposta,
alarmes vermelhos, datastores quase cheios ou inacessíveis), depois os
anfitriões com a sua carga, os datastores com o seu nível de ocupação, e as VM
por estado de energia.

## Como foi validado {#how-it-was-validated}

Contra o simulador govmomi `vcsim` 0.56.0, em modo vCenter e em modo ESXi:
início de sessão e palavra-passe recusada, reutilização e expiração de sessão,
paginação do inventário, um anfitrião sem resposta, um anfitrião em modo de
manutenção, uma VM desligada e um modelo. O simulador não levanta nenhum
alarme: a leitura dos alarmes disparados é validada contra a estrutura
`AlarmState` documentada da Web Services API.

## Criar um utilizador só de leitura no vSphere {#create-a-read-only-user-in-vsphere}

1. Num vCenter: Administration → Single Sign On → Users and Groups → Users, domínio vsphere.local → Add. Dê ao utilizador o nome seguinte e uma palavra-passe longa e aleatória.

    ```
    dumbmonit
    ```

2. Ainda no vCenter: Administration → Access Control → Global Permissions → Add. Escolha esse utilizador, o papel Read-only, e marque Propagate to children. O Read-only vê todos os anfitriões, VM, datastores e alarmes, e não pode alterar nada.

3. Num anfitrião ESXi autónomo, em vez disso: no cliente do anfitrião, Manage → Security & users → Users → Add user, com o mesmo nome. Depois Host → Actions → Permissions → Add user: escolha-o, o papel Read-only, e marque Propagate to all children.

4. No DumbMonit, introduza o endereço do vCenter ou do anfitrião, por exemplo «vcenter.lan» ou «esxi1.lan», com o nome de utilizador tal como foi criado: dumbmonit@vsphere.local num vCenter, dumbmonit num anfitrião. Quando um vCenter gere os anfitriões, adicione apenas o vCenter: ele reporta todos os anfitriões, VM e datastores.

5. O DumbMonit só lê, através da vSphere Web Services API (/sdk). Nunca liga nem desliga uma VM, nunca entra em modo de manutenção e nunca reconhece um alarme.

!!! warning
    Não reutilize a conta do domínio de início de sessão único nem a conta incorporada do anfitrião: o papel Read-only acima é tudo o que o DumbMonit precisa. O vCenter e o ESXi usam por predefinição um certificado autoassinado: ative «Aceitar um certificado não verificável» a menos que o anfitrião do DumbMonit confie na sua autoridade de certificação.

## Credenciais {#credentials}

| Credencial | Campos |
|---|---|
| Nome de utilizador / palavra-passe | O utilizador dedicado com o papel Read-only: `dumbmonit@vsphere.local` num vCenter, `dumbmonit` num anfitrião. |

Endereço: um nome de anfitrião ou IP (`vcenter.lan`), `host:port`, ou um URL
completo; um `/sdk` ou `/ui` no fim é aceite. Opções: porta (443), verificação
do certificado, tempo limite do pedido (20 s) e alarmes disparados (ler).

## Limites conhecidos

- A expiração das licenças não é lida: a função só de leitura não vê as licenças.
- No máximo 1000 VM e 64 alarmes são descritos um a um.
- Sem contadores de desempenho por VM (processador, memória, latência de disco): o DumbMonit lê o inventário e os seus estados.
- As ventoinhas, as fontes de alimentação e as temperaturas só são vistas através da saúde global de um host e dos alarmes que o vSphere emite.
- Os snapshots e a consolidação foram construídos a partir da estrutura documentada da API e ainda não foram verificados num vCenter real.
