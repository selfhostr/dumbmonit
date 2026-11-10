# Unraid

Сервер Unraid через его GraphQL API: неначатый массив, диски отключённые,
отсутствующие или сообщающие об ошибках чтения, проверки паритета, нашедшие
ошибки или не выполнявшиеся неделями, пулы кэша, заполняющиеся контейнеры,
которые должны работать и не работают, и состояние ВМ.

DumbMonit отправляет ключ API с ролью Viewer в заголовке `x-api-key` на
`/graphql`. Роль Viewer читает всё и не может ничего изменить. API встроен в
Unraid начиная с 7.2; на версиях 6.12–7.1 он идёт с плагином Unraid Connect.

Каждую минуту DumbMonit отправляет один обязательный запрос (версия, массив,
паритет, диски данных и кэша) и четыре опциональных, каждый отдельно, так
что поле, которого нет в более старой версии API, или выключенные ВМ,
убирают только эту часть: проверку паритета, контейнеры Docker, ВМ и
непрочитанные уведомления.

!!! warning "Проверено только по документации производителя — пока не испытано на реальной системе"

    Эта интеграция построена на документации API Unraid и опубликованной
    GraphQL-схеме API Unraid (имена полей, перечисления, размеры в
    килобайтах), а не на работающем сервере. Каждый запрос — это чтение.
    Сообщите нам, если что-то не работает.

## Что отслеживается

Все метрики с префиксом `dumbmonit_unraid_`.

| Метрика | Метки | Значение |
|---|---|---|
| `version_info` | `version` | Значение 1: версия Unraid. |
| `array_started` | | 1, когда массив запущен. |
| `array_state_info` | `state` | Значение 1: `started`, `stopped`, `new_array`, `recon_disk`, `disable_disk`… |
| `array_size_bytes`, `array_used_bytes`, `array_free_bytes`, `array_used_percent` | | Ёмкость массива. |
| `disk_ok` | `disk`, `role` (`parity`, `data`, `cache`) | 1, когда статус диска — `DISK_OK`. Пустые отсеки не учитываются. |
| `disk_status_info` | `disk`, `role`, `status` | Значение 1: статус Unraid (`disk_ok`, `disk_dsbl`, `disk_np_missing`, `disk_invalid`…). |
| `disk_errors` | `disk`, `role` | Ошибки чтения с момента запуска массива (колонка Errors). |
| `disk_temperature_celsius` | `disk`, `role` | Только пока диск вращается: Unraid не сообщает температуру для спящего диска. |
| `disk_spinning` | `disk`, `role` | 1, когда диск вращается. |
| `disk_size_bytes`, `disk_fs_size_bytes`, `disk_used_percent` | `disk`, `role` | Размер, размер файловой системы и заполнение, для дисков с файловой системой. |
| `disks`, `disks_problem`, `disks_with_errors` | | Счётчики. |
| `parity_check_status_info` | `status` | Значение 1: `completed`, `running`, `paused`, `cancelled`, `failed`, `never_run`. |
| `parity_check_running`, `parity_check_progress_percent` | | Проверка в процессе. |
| `parity_check_ok` | | После завершённой проверки: 1 без ошибок синхронизации, 0 с ошибками или при отказе. Отменённая проверка ничего не доказывает и ничего не устанавливает. |
| `parity_check_errors`, `parity_check_age_seconds`, `parity_check_duration_seconds` | | Последняя проверка. |
| `containers`, `containers_running`, `containers_autostart_stopped` | | Контейнеры Docker. |
| `container_running`, `container_autostart_stopped` | `container` | На контейнер; второе равно 1, когда контейнер, настроенный на запуск с массивом, не работает. |
| `vms`, `vms_running` | | ВМ. |
| `vm_state_info`, `vm_crashed` | `vm` (и `state`) | На ВМ. |
| `notifications_unread` | `importance`: `alert`, `warning`, `info` | Непрочитанные уведомления. |
| `registration_ok` | | 1, когда Unraid не сообщает о проблемах с лицензией, 0, когда состояние лицензии является ошибкой (истёкший пробный период, ключ не соответствует флеш-накопителю, отсутствует файл ключа…). |
| `registration_info` | `state`, `type` | Значение 1: состояние лицензии, как его сообщает Unraid (`trial`, `pro`, `eexpired`, `eguid`…), и тип лицензии. |
| `trial_expiry_seconds` | | До окончания пробной лицензии; только во время пробного периода. |
| `os_updates_expiry_seconds` | | До окончания права лицензии на обновления Unraid OS; отсутствует, когда у лицензии нет такой даты. |
| `uptime_seconds` | | С момента запуска сервера. |
| `cpu_usage_percent` | | Загрузка процессора, все ядра вместе. |
| `memory_total_bytes`, `memory_available_bytes`, `memory_used_percent` | | Память. Занятой считается та, что недоступна: кэш, который Unraid может вернуть, не учитывается. |
| `swap_used_percent` | | Только когда у сервера есть swap. |

Лицензия и нагрузка на систему (время работы, процессор, память) берутся из двух дополнительных необязательных запросов. Более старая версия API, которая их не знает, просто оставляет эти строки пустыми: остальная часть монитора не затрагивается.

Применяемые [встроенные правила](../alerting/rules.md#unraid):

| Правило | Что | Порог по умолчанию | Выдержка | Важность | Напоминание |
|---|---|---|---|---|---|
| Unraid array stopped | Массив Unraid не запущен: шары, контейнеры и ВМ, расположенные на нём, недоступны (`dumbmonit_unraid_array_started`). | < 1 | 10 мин | Warning (`critical`) | 6 ч |
| Unraid disk disabled or missing | Диск Unraid отключён, отсутствует или недействителен: массив работает в деградированном режиме, и ещё один отказ приведёт к потере данных (`dumbmonit_unraid_disk_ok`). | < 1 | 5 мин | Warning (`critical`) | 6 ч |
| Unraid disk read errors | Диск Unraid сообщал об ошибках чтения с момента запуска массива: при следующей он может быть отключён (`dumbmonit_unraid_disk_errors`). | > 0 | 5 мин | Advisory (`warning`) | 1 дн |
| Unraid disk too hot | Диск массива или чётности Unraid остаётся выше 55 °C пятнадцать минут (`dumbmonit_unraid_disk_temperature_celsius{role=~"data\|parity"}`). | > 55 °C | 15 мин | Advisory (`warning`) | 6 ч |
| Unraid parity check found errors | Последняя проверка чётности Unraid не удалась или нашла ошибки синхронизации: чётность больше не соответствует данным (`dumbmonit_unraid_parity_check_ok`). | < 1 | 5 мин | Advisory (`warning`) | 1 дн |
| Unraid parity check overdue | Последней проверке чётности Unraid более 40 дней: ежемесячная проверка пропущена (`dumbmonit_unraid_parity_check_age_seconds`). | > 40 дн | 1 ч | Advisory (`warning`) | 1 дн |
| Unraid cache nearly full | Кэш-пул Unraid заполнен более чем на 90 %: запись откатывается на медленный массив или завершается ошибкой (`dumbmonit_unraid_disk_used_percent{role="cache"}`). | > 90 % | 30 мин | Advisory (`warning`) | 1 дн |
| Unraid container stopped | Контейнер Docker, настроенный на запуск вместе с массивом, не запущен (`dumbmonit_unraid_container_autostart_stopped`). | > 0 | 10 мин | Advisory (`warning`) | 6 ч |
| Unraid VM crashed | Unraid сообщает, что виртуальная машина аварийно завершилась (crashed) (`dumbmonit_unraid_vm_crashed`). | > 0 | 5 мин | Advisory (`warning`) | 6 ч |
| Unraid alert unread | Unraid выдал уведомление высшей важности (alert), которое никто не прочитал: отказавший диск, перегрев, ошибка массива… (`dumbmonit_unraid_notifications_unread{importance="alert"}`). | > 0 | 10 мин | Advisory (`warning`) | 24 ч |
| Unraid array nearly full | Массив заполнен более чем на 95 %: запись на общие ресурсы скоро начнёт завершаться ошибкой. Снимается ниже 93 % (`dumbmonit_unraid_array_used_percent`). | > 95 % | 1 ч | Advisory (`warning`) | 24 ч |
| Unraid memory nearly exhausted | Уже пятнадцать минут занято более 95 % памяти: Unraid работает из памяти, и когда она заканчивается, контейнеры или ВМ принудительно завершаются. Снимается ниже 90 % (`dumbmonit_unraid_memory_used_percent`). | > 95 % | 15 мин | Advisory (`warning`) | 6 ч |
| Unraid license problem | Unraid сообщает о проблеме с лицензией (истёкший пробный период, ключ не соответствует флеш-накопителю, отсутствует файл ключа…): массив может отказаться запускаться после следующей остановки (`dumbmonit_unraid_registration_ok`). | < 1 | 30 мин | Advisory (`warning`) | 24 ч |
| Unraid trial ending | Пробная лицензия заканчивается менее чем через три дня: после этого массив больше не запускается (`dumbmonit_unraid_trial_expiry_seconds`). | < 3 дн | 1 ч | Advisory (`warning`) | 24 ч |
| Unraid OS updates eligibility ending | Право лицензии на обновления Unraid OS заканчивается менее чем через 30 дней. Сервер продолжает работать; он лишь перестаёт получать новые версии (`dumbmonit_unraid_os_updates_expiry_seconds`). | < 30 дн | 1 ч | Info (`info`) | 7 дн |

## Страница монитора

Панель Array сообщает, что не так, в одном предложении, показывает
заполнение массива, последнюю проверку паритета, работающие контейнеры и ВМ,
затем перечисляет каждый диск — сначала паритет, затем данные, затем кэш — с
его статусом, температурой и заполнением, и контейнеры, которые должны
работать.

## Создание ключа API только для чтения для DumbMonit

1. На Unraid 7.2 или новее API встроен. На Unraid 6.12–7.1 сначала установите плагин Unraid Connect из вкладки Apps: он даёт тот же API, и для локального использования вход в Unraid Connect не нужен.

2. Создайте ключ API с ролью Viewer, которая читает всё и не может ничего изменить: в Settings > Management Access > API Keys, или из терминала Unraid.

    ```
    unraid-api apikey --create --name dumbmonit --roles VIEWER --description "DumbMonit monitoring" --json
    ```

3. Скопируйте ключ. В DumbMonit введите адрес сервера Unraid, например «tower.lan», и вставьте ключ как API key. Если веб-интерфейс обслуживается по HTTPS, выберите https как протокол.

!!! warning
    Unraid не сообщает температуру для диска, который остановлен, и DumbMonit никогда не разбудит его: у спящего диска просто нет температуры, пока он не завращается.

## Учётные данные

| Учётные данные | Поля |
|---|---|
| Ключ API | Ключ с ролью Viewer, отправляется как `x-api-key`. |

Адрес: имя хоста или IP сервера Unraid, `host:port`, или URL. Опции:

- **Protocol** (`http`) и **Port** (80): те, что у веб-интерфейса. С SSL,
  установленным на Strict в Settings > Management Access, используйте
  `https`, и адрес `myunraid.net`, если сертификат именно такой.
- **Accept an unverifiable certificate**: для самоподписанного сертификата.
- **Timeout per request** (15 с).

## Известные ограничения

- ИБП не читается: подключите его через [NUT](nut.md).
- Без подробностей SMART: DumbMonit читает состояние, ошибки и температуру, которые Unraid сообщает для каждого диска, но не атрибуты SMART.
- Не читается, ожидает ли контейнер обновления образа.
- Значениям лицензии, процессора и памяти нужна свежая версия API Unraid. На версии, в которой их нет, эти строки и их правила просто отсутствуют.

## Диагностика

«Unraid refused the API key (401)»: ключ неверен или был удалён.

«Unraid accepted the API key but refused to answer (403)»: у ключа нет роли,
либо роль более узкая, чем Viewer.

Нет данных по контейнерам, ВМ или паритету: этот опциональный запрос не
удался (ВМ выключены, более старая версия API без этого поля); остальное
всё равно читается. Причина — в журнале сервера на уровне debug.
