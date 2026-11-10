# pfSense

Файрвол pfSense через пакет community REST API
(`pfSense-pkg-RESTAPI`, версия 2): шлюзы, которые недоступны, теряют пакеты
или отвечают медленно, включённые интерфейсы без линка, включённые службы,
которые остановились, и CPU, память, диск и температура.

DumbMonit отправляет ключ API в заголовке `X-API-Key`. Ключ несёт ровно те
права, что у пользователя pfSense, которому он принадлежит: пользователь
`dumbmonit` ниже обладает только правами `GET` пяти страниц, которые читает
DumbMonit, так что ключ может их читать и ничего не менять. Каждую минуту
DumbMonit параллельно читает `/api/v2/status/system`,
`/api/v2/status/gateways`, `/api/v2/status/interfaces`,
`/api/v2/status/services` и `/api/v2/system/version`, и, если дано право,
`/api/v2/system/restapi/version`.

!!! warning "Проверено только по документации производителя — пока не испытано на реальной системе"

    Эта интеграция построена на опубликованной документации пакета REST API
    и исходниках его моделей (имена полей, единицы измерения, обёртка
    ответа и её идентификаторы ошибок), а не на работающем pfSense. Каждый
    вызов — это чтение. Сообщите нам, если что-то не работает.

## Что отслеживается

Все метрики с префиксом `dumbmonit_pfsense_`.

| Метрика | Метки | Значение |
|---|---|---|
| `cpu_usage_percent`, `memory_used_percent`, `swap_used_percent`, `disk_used_percent`, `mbuf_used_percent` | | Как показывает панель, в процентах. Показатель CPU — собственная оценка pfSense по средней загрузке. |
| `temperature_celsius` | | Когда pfSense может прочитать датчик. |
| `load_average` | `period`: `1m`, `5m`, `15m` | Средняя загрузка. |
| `cpu_count` | | Ядра процессора. |
| `version_info` | `version` | Значение 1: версия pfSense, `2.7.2-RELEASE`. |
| `gateway_up` | `gateway` | 0, когда pfSense отмечает шлюз как недоступный, иначе 1. |
| `gateway_status_info` | `gateway`, `status` | Значение 1: собственное слово pfSense (`none`, когда всё в порядке, `down`, `loss`, `delay`). |
| `gateway_loss_percent` | `gateway` | Потери пакетов до адреса монитора. |
| `gateway_delay_milliseconds`, `gateway_stddev_milliseconds` | `gateway` | Время приёма-передачи до адреса монитора и его отклонение; отсутствует, пока шлюз недоступен. |
| `gateways`, `gateways_down` | | Счётчики. |
| `interface_enabled`, `interface_up` | `interface`, `descr` | Включён в конфигурации; линк активен. |
| `interface_down` | `interface`, `descr` | 1, когда интерфейс включён и не имеет линка. Отключённый интерфейс никогда не считается недоступным. |
| `interface_bytes_in_total`, `interface_bytes_out_total`, `interface_errors_in_total`, `interface_errors_out_total` | `interface`, `descr` | Счётчики. |
| `interfaces_down` | | Счётчик. |
| `service_running`, `service_enabled` | `service`, `description` | Как показывает Status > Services. |
| `service_stopped` | `service`, `description` | 1, когда служба включена и не работает. |
| `services_stopped` | | Счётчик. |
| `states`, `states_max`, `states_used_percent` | | Соединения, которые отслеживает pf, предел и занятая доля. Требуется привилегия `/api/v2/firewall/states/size GET`. |
| `carp_enabled`, `carp_maintenance_mode` | | 1, когда CARP включён; 1, когда он переведён в постоянный режим обслуживания. Требуется привилегия `/api/v2/status/carp GET`. |
| `restapi_update_available` | | С правом `/api/v2/system/restapi/version`: 1, когда у пакета REST API есть более новый релиз. |
| `restapi_version_info` | `version`, `latest` | Значение 1: установленный и последний релиз пакета. |

pfSense не сообщает через этот API, доступно ли обновление самого pfSense:
единственный маршрут, касающийся системных обновлений
(`/api/v2/system/update`), запускает его. DumbMonit сообщает только об
обновлениях самого пакета.

Таблица состояний и CARP — необязательные запросы. Каждому нужна ещё одна привилегия у пользователя `dumbmonit`: `/api/v2/firewall/states/size GET` и `/api/v2/status/carp GET`. Без привилегии pfSense отклоняет этот запрос, и соответствующие значения просто отсутствуют: ни ошибки, ни оповещения.

Применяемые [встроенные правила](../alerting/rules.md#pfsense):

| Правило | Что | Порог по умолчанию | Выдержка | Важность | Напоминание |
|---|---|---|---|---|---|
| pfSense gateway down | Шлюз pfSense не работает: его адрес мониторинга больше не отвечает, и несомый им трафик прекращается или переключается (`dumbmonit_pfsense_gateway_up`). | < 1 | 3 мин | Warning (`critical`) | 6 ч |
| pfSense gateway losing packets | Шлюз pfSense теряет более 10 % зондов к своему адресу мониторинга: линия деградировала (`dumbmonit_pfsense_gateway_loss_percent unless on (target, gateway) dumbmonit_pfsense_gateway_up == 0`). | > 10 % | 10 мин | Advisory (`warning`) | 6 ч |
| pfSense gateway slow | Время кругового обхода до адреса мониторинга шлюза pfSense остаётся выше 500 мс пятнадцать минут (`dumbmonit_pfsense_gateway_delay_milliseconds`). | > 500 мс | 15 мин | Advisory (`warning`) | 6 ч |
| pfSense interface without link | У включённого интерфейса pfSense нет линка: кабель, порт коммутатора или сетевая карта (`dumbmonit_pfsense_interface_down`). | > 0 | 5 мин | Advisory (`warning`) | 6 ч |
| pfSense service stopped | Включённая служба pfSense не запущена: DNS-резолвер, VPN, NTP… (`dumbmonit_pfsense_service_stopped`). | > 0 | 5 мин | Advisory (`warning`) | 6 ч |
| pfSense disk nearly full | Диск pfSense заполнен более чем на 90 %: логи, отчёты и обновления пакетов перестают записываться (`dumbmonit_pfsense_disk_used_percent`). | > 90 % | 30 мин | Advisory (`warning`) | 1 дн |
| pfSense state table nearly full | pf отслеживает более 80 % разрешённых ему соединений: за пределом межсетевой экран отбрасывает новые соединения без каких-либо других симптомов. Снимается ниже 70 % (`dumbmonit_pfsense_states_used_percent`). | > 80 % | 10 мин | Advisory (`warning`) | 6 ч |
| pfSense network buffers nearly exhausted | Занято более 90 % сетевых буферов: межсетевой экран, у которого они закончились, перестаёт пересылать трафик, хотя его процессор простаивает. Снимается ниже 80 % (`dumbmonit_pfsense_mbuf_used_percent`). | > 90 % | 10 мин | Advisory (`warning`) | 6 ч |
| pfSense temperature high | Температура, которую считывает pfSense, десять минут держалась выше 85 °C: устройство вот-вот начнёт само себя замедлять. Снимается при 78 °C (`dumbmonit_pfsense_temperature_celsius`). | > 85 °C | 10 мин | Advisory (`warning`) | 6 ч |
| pfSense CPU high | Процессор пятнадцать минут держался выше 90 %. Снимается ниже 80 % (`dumbmonit_pfsense_cpu_usage_percent`). | > 90 % | 15 мин | Advisory (`warning`) | 6 ч |
| pfSense swap in use | pfSense уже час использует больше половины своего swap: ему не хватает памяти (`dumbmonit_pfsense_swap_used_percent`). | > 50 % | 1 ч | Advisory (`warning`) | 24 ч |
| pfSense interface errors | Интерфейс зарегистрировал более 100 ошибок приёма или передачи за пятнадцать минут: проверьте кабель, SFP-модуль и настройку дуплекса (`dumbmonit_pfsense_interface_errors_in_total` + `dumbmonit_pfsense_interface_errors_out_total`). | > 100 | 15 мин | Advisory (`warning`) | 24 ч |
| pfSense CARP maintenance mode left on | CARP уже час оставлен в постоянном режиме обслуживания: этот межсетевой экран передал свои виртуальные адреса напарнику и сам их не вернёт (`dumbmonit_pfsense_carp_maintenance_mode`). | > 0 | 1 ч | Info (`info`) | 24 ч |

## Страница монитора

Панель Firewall сообщает, что не так, в одном предложении, показывает CPU,
память, диск и температуру, затем перечисляет каждый шлюз с его задержкой и
потерями, интерфейсы без линка, остановленные службы, и доступное обновление
пакета REST API.

## Создание ключа REST API только для чтения для DumbMonit

1. Установите пакет community REST API (pfSense-pkg-RESTAPI, версия 2) из Diagnostics > Command Prompt или оболочки SSH, с файлом со страницы релизов проекта, соответствующим вашей версии pfSense (2.7.2 ниже). Затем, в System > REST API > Settings, отметьте и BasicAuth, и Key среди методов аутентификации.

    ```
    pkg-static add https://github.com/pfrest/pfSense-pkg-RESTAPI/releases/latest/download/pfSense-2.7.2-pkg-RESTAPI.pkg
    ```

2. В System > User Manager добавьте пользователя с именем dumbmonit с длинным случайным паролем. В Effective Privileges добавьте только эти права REST API: /api/v2/status/system GET, /api/v2/status/gateways GET, /api/v2/status/interfaces GET, /api/v2/status/services GET, /api/v2/system/version GET, плюс /api/v2/system/restapi/version GET, чтобы узнавать об обновлениях самого пакета. Только для следующего шага добавьте также /api/v2/auth/key POST.

3. Создайте ключ как dumbmonit, с любой машины, достигающей pfSense: ключ несёт ровно права этого пользователя. Скопируйте значение ключа из ответа, затем снова удалите право /api/v2/auth/key POST.

    ```
    curl -k -u dumbmonit -X POST -H 'Content-Type: application/json' -d '{"descr":"DumbMonit"}' https://pfsense.lan/api/v2/auth/key
    ```

4. В DumbMonit введите адрес pfSense, например «pfsense.lan», и вставьте ключ как API key. pfSense поставляется с самоподписанным сертификатом: отметьте Accept an unverifiable certificate, если вы его не заменили.

!!! warning
    REST API не сообщает, доступно ли обновление pfSense, только есть ли оно у самого пакета. Пакет — это проект сообщества, а не часть pfSense: переустанавливайте его после каждого обновления pfSense.

## Учётные данные

| Учётные данные | Поля |
|---|---|
| Ключ REST API | Ключ, созданный для `dumbmonit` на шаге 3, отправляется как `X-API-Key`. |

Адрес: имя хоста или IP pfSense, `host:port`, или URL. Опции:

- **Protocol** (`https`) и **Port** (443): те, что у webConfigurator,
  задаются в System > Advanced > Admin Access.
- **Accept an unverifiable certificate**: собственный сертификат pfSense
  самоподписан.
- **Timeout per request** (15 с).

## Известные ограничения

- Сертификаты не читаются: пакет возвращает вместе с ними их закрытые ключи, и DumbMonit отказывается их запрашивать. Следите за сроком действия с помощью [монитора TLS](services.md#tls) на адресе, который отдаёт сертификат.
- VPN-туннели (IPsec, OpenVPN, WireGuard) не отслеживаются по одному. Их службы отслеживаются — правилом «pfSense service stopped».
- Об обновлениях pfSense не сообщается: единственный маршрут API, который их касается, запускает обновление.
- Для памяти правила нет: pfSense считает кэш ZFS занятой памятью, поэтому высокое значение — норма. Занятый swap — надёжный признак того, что межсетевому экрану не хватает памяти.

## Диагностика

«pfSense refused the API key (401)»: ключ неверен, либо Key не отмечен среди
методов аутентификации в System > REST API > Settings.

«pfSense accepted the API key but refused … (403)»: пользователю `dumbmonit`
не хватает права для этой страницы; сообщение называет его.

«… not found: is the REST API package installed?»: пакет отсутствует,
обычно после обновления pfSense, которое его удаляет. Переустановите его,
как на шаге 1.
