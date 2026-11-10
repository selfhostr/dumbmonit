# pfSense {#pfsense}

Uma firewall pfSense através do pacote REST API da comunidade
(`pfSense-pkg-RESTAPI`, versão 2): gateways em baixo, a perder pacotes ou a
responder lentamente, interfaces ativadas sem ligação, serviços ativados
que pararam, e CPU, memória, disco e temperatura.

O DumbMonit envia uma chave de API no cabeçalho `X-API-Key`. Uma chave
transporta exactamente os privilégios do utilizador pfSense a que
pertence: o utilizador `dumbmonit` abaixo detém só os privilégios `GET` das
cinco páginas que o DumbMonit lê, pelo que a chave pode ler essas páginas e
não alterar nada. A cada minuto o DumbMonit lê, em paralelo,
`/api/v2/status/system`, `/api/v2/status/gateways`,
`/api/v2/status/interfaces`, `/api/v2/status/services` e
`/api/v2/system/version`, e, se o privilégio foi dado,
`/api/v2/system/restapi/version`.

!!! warning "Validado apenas contra a documentação do fornecedor — ainda não testado num sistema real"

    Esta integração foi construída a partir da documentação publicada do
    pacote REST API e do código-fonte dos seus modelos (nomes de campos,
    unidades, o envelope de resposta e os seus identificadores de erro),
    não contra um pfSense em funcionamento. Toda chamada é uma leitura.
    Diga-nos o que falha.

## O que monitoriza {#what-it-watches}

Todas as métricas têm o prefixo `dumbmonit_pfsense_`.

| Métrica | Etiquetas | Significado |
|---|---|---|
| `cpu_usage_percent`, `memory_used_percent`, `swap_used_percent`, `disk_used_percent`, `mbuf_used_percent` | | Como o painel os mostra, em percentagem. O valor de CPU é a própria estimativa do pfSense a partir da média de carga. |
| `temperature_celsius` | | Quando o pfSense consegue ler um sensor. |
| `load_average` | `period`: `1m`, `5m`, `15m` | Média de carga. |
| `cpu_count` | | Núcleos do processador. |
| `version_info` | `version` | Valor 1: a versão do pfSense, `2.7.2-RELEASE`. |
| `gateway_up` | `gateway` | 0 quando o pfSense marca o gateway como em baixo, 1 caso contrário. |
| `gateway_status_info` | `gateway`, `status` | Valor 1: a própria palavra do pfSense (`none` quando tudo está bem, `down`, `loss`, `delay`). |
| `gateway_loss_percent` | `gateway` | Perda de pacotes até o endereço de monitorização. |
| `gateway_delay_milliseconds`, `gateway_stddev_milliseconds` | `gateway` | Tempo de ida e volta até o endereço de monitorização e o seu desvio; ausente enquanto o gateway está em baixo. |
| `gateways`, `gateways_down` | | Contagens. |
| `interface_enabled`, `interface_up` | `interface`, `descr` | Ativada na configuração; ligação ativa. |
| `interface_down` | `interface`, `descr` | 1 quando a interface está ativada e não tem ligação. Uma interface desativada nunca está em baixo. |
| `interface_bytes_in_total`, `interface_bytes_out_total`, `interface_errors_in_total`, `interface_errors_out_total` | `interface`, `descr` | Contadores. |
| `interfaces_down` | | Contagem. |
| `service_running`, `service_enabled` | `service`, `description` | Como Status > Services os mostra. |
| `service_stopped` | `service`, `description` | 1 quando o serviço está ativado e não está em execução. |
| `services_stopped` | | Contagem. |
| `states`, `states_max`, `states_used_percent` | | Ligações que o pf segue, o limite e a parte em uso. Precisa do privilégio `/api/v2/firewall/states/size GET`. |
| `carp_enabled`, `carp_maintenance_mode` | | 1 quando o CARP está ativado; 1 quando foi posto em modo de manutenção persistente. Precisa do privilégio `/api/v2/status/carp GET`. |
| `restapi_update_available` | | Com o privilégio `/api/v2/system/restapi/version`: 1 quando o pacote REST API tem uma versão mais recente. |
| `restapi_version_info` | `version`, `latest` | Valor 1: versão instalada e mais recente do pacote. |

O pfSense não diz através desta API se uma atualização do pfSense está
disponível: a única rota que toca nas atualizações do sistema
(`/api/v2/system/update`) inicia uma. O DumbMonit só reporta as
atualizações do próprio pacote.

A tabela de estados e o CARP são leituras opcionais. Cada uma precisa de mais um privilégio no utilizador `dumbmonit`: `/api/v2/firewall/states/size GET` e `/api/v2/status/carp GET`. Sem o privilégio, o pfSense recusa essa leitura e os valores correspondentes ficam simplesmente ausentes: nem erro, nem alerta.

As [regras incorporadas](../alerting/rules.md#pfsense) que se aplicam:

| Regra | O quê | Limiar predefinido | Retenção | Severidade | Lembrete |
|---|---|---|---|---|---|
| pfSense gateway down | Um gateway pfSense está em baixo: o seu endereço de monitorização já não responde, e o tráfego que transporta para ou falha para outro (`dumbmonit_pfsense_gateway_up`). | < 1 | 3 min | Warning (`critical`) | 6 h |
| pfSense gateway losing packets | Um gateway pfSense perde mais de 10% das sondas ao seu endereço de monitorização: a linha está degradada (`dumbmonit_pfsense_gateway_loss_percent unless on (target, gateway) dumbmonit_pfsense_gateway_up == 0`). | > 10 % | 10 min | Advisory (`warning`) | 6 h |
| pfSense gateway slow | O tempo de ida e volta até ao endereço de monitorização de um gateway pfSense manteve-se acima de 500 ms durante quinze minutos (`dumbmonit_pfsense_gateway_delay_milliseconds`). | > 500 ms | 15 min | Advisory (`warning`) | 6 h |
| pfSense interface without link | Uma interface pfSense ativada não tem ligação: cabo, porta de comutador ou placa de rede (`dumbmonit_pfsense_interface_down`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| pfSense service stopped | Um serviço pfSense ativado não está em execução: resolvedor DNS, VPN, NTP… (`dumbmonit_pfsense_service_stopped`). | > 0 | 5 min | Advisory (`warning`) | 6 h |
| pfSense disk nearly full | O disco do pfSense está mais de 90% cheio: logs, relatórios e atualizações de pacote deixam de ser escritos (`dumbmonit_pfsense_disk_used_percent`). | > 90 % | 30 min | Advisory (`warning`) | 1 d |
| pfSense state table nearly full | O pf segue mais de 80 % das ligações que lhe são permitidas: passado o limite, a firewall descarta as novas ligações sem qualquer outro sintoma. Resolve abaixo de 70 % (`dumbmonit_pfsense_states_used_percent`). | > 80 % | 10 min | Advisory (`warning`) | 6 h |
| pfSense network buffers nearly exhausted | Mais de 90 % dos buffers de rede estão em uso: uma firewall que fica sem eles deixa de encaminhar tráfego enquanto o seu processador ainda está parado. Resolve abaixo de 80 % (`dumbmonit_pfsense_mbuf_used_percent`). | > 90 % | 10 min | Advisory (`warning`) | 6 h |
| pfSense temperature high | A temperatura que o pfSense lê manteve-se acima de 85 °C durante dez minutos: o equipamento está prestes a abrandar-se a si próprio. Resolve aos 78 °C (`dumbmonit_pfsense_temperature_celsius`). | > 85 °C | 10 min | Advisory (`warning`) | 6 h |
| pfSense CPU high | O processador manteve-se acima de 90 % durante quinze minutos. Resolve abaixo de 80 % (`dumbmonit_pfsense_cpu_usage_percent`). | > 90 % | 15 min | Advisory (`warning`) | 6 h |
| pfSense swap in use | O pfSense usa mais de metade do seu swap há uma hora: tem falta de memória (`dumbmonit_pfsense_swap_used_percent`). | > 50 % | 1 h | Advisory (`warning`) | 24 h |
| pfSense interface errors | Uma interface registou mais de 100 erros de receção ou de transmissão em quinze minutos: verifique o cabo, o módulo SFP e a definição de duplex (`dumbmonit_pfsense_interface_errors_in_total` + `dumbmonit_pfsense_interface_errors_out_total`). | > 100 | 15 min | Advisory (`warning`) | 24 h |
| pfSense CARP maintenance mode left on | O CARP ficou em modo de manutenção persistente durante uma hora: esta firewall entregou os seus endereços virtuais ao parceiro e não os retoma sozinha (`dumbmonit_pfsense_carp_maintenance_mode`). | > 0 | 1 h | Info (`info`) | 24 h |

## A página do monitor {#the-device-page}

O painel Firewall diz o que está mal numa frase, mostra CPU, memória, disco
e temperatura, depois lista cada gateway com a sua latência e perda, as
interfaces sem ligação, os serviços parados, e uma atualização disponível
do pacote REST API.

## Criar uma chave REST API só de leitura para o DumbMonit {#create-a-read-only-rest-api-key-for-dumbmonit}

1. Instale o pacote REST API da comunidade (pfSense-pkg-RESTAPI, versão 2) a partir de Diagnostics > Command Prompt ou de uma shell SSH, com o ficheiro da página de versões do projeto que corresponde à sua versão do pfSense (2.7.2 abaixo). Depois, em System > REST API > Settings, marque tanto BasicAuth como Key entre os métodos de autenticação.

    ```
    pkg-static add https://github.com/pfrest/pfSense-pkg-RESTAPI/releases/latest/download/pfSense-2.7.2-pkg-RESTAPI.pkg
    ```

2. Em System > User Manager, adicione um utilizador chamado dumbmonit com uma senha longa e aleatória. Em Effective Privileges, adicione só estes privilégios REST API: /api/v2/status/system GET, /api/v2/status/gateways GET, /api/v2/status/interfaces GET, /api/v2/status/services GET, /api/v2/system/version GET, mais /api/v2/system/restapi/version GET para saber das próprias atualizações do pacote. Só para o passo seguinte, adicione também /api/v2/auth/key POST.

3. Crie a chave como dumbmonit, a partir de qualquer máquina que alcance o pfSense: a chave transporta exactamente os privilégios desse utilizador. Copie o valor da chave na resposta, depois remova de novo o privilégio /api/v2/auth/key POST.

    ```
    curl -k -u dumbmonit -X POST -H 'Content-Type: application/json' -d '{"descr":"DumbMonit"}' https://pfsense.lan/api/v2/auth/key
    ```

4. No DumbMonit, introduza o endereço do pfSense, por exemplo «pfsense.lan», e cole a chave como API key. O pfSense vem com um certificado autoassinado: marque Accept an unverifiable certificate a menos que o tenha substituído.

!!! warning
    A REST API não diz se uma atualização do pfSense está disponível, só se o próprio pacote tem uma. O pacote é um projeto da comunidade, não parte do pfSense: reinstale-o depois de cada atualização do pfSense.

## Credenciais {#credentials}

| Credencial | Campos |
|---|---|
| Chave REST API | A chave criada para `dumbmonit` no passo 3, enviada como `X-API-Key`. |

Endereço: o nome de anfitrião ou IP do pfSense, `anfitrião:porta`, ou um
URL. Opções:

- **Protocol** (`https`) e **Port** (443): os do webConfigurator, definidos
  em System > Advanced > Admin Access.
- **Accept an unverifiable certificate**: o próprio certificado do pfSense
  é autoassinado.
- **Timeout per request** (15 s).

## Limites conhecidos

- Os certificados não são lidos: o pacote devolve as suas chaves privadas juntamente com eles, e o DumbMonit recusa-se a pedi-los. Vigie uma data de expiração com um [monitor TLS](services.md#tls) no endereço que serve o certificado.
- Os túneis VPN (IPsec, OpenVPN, WireGuard) não são seguidos um a um. Os seus serviços são, através de «pfSense service stopped».
- As atualizações do pfSense não são indicadas: a única rota da API que lhes toca inicia uma.
- A memória não tem regra: o pfSense conta a cache ZFS como memória usada, por isso um valor alto é normal. O swap em uso é o sinal fiável de uma firewall com falta de memória.

## Resolução de problemas {#troubleshooting}

«pfSense refused the API key (401)»: a chave está errada, ou Key não está
marcado entre os métodos de autenticação em System > REST API > Settings.

«pfSense accepted the API key but refused … (403)»: o utilizador
`dumbmonit` não tem o privilégio dessa página; a mensagem nomeia-o.

«… not found: is the REST API package installed?»: o pacote está em falta,
tipicamente depois de uma atualização do pfSense, que o remove.
Reinstale-o como no passo 1.
