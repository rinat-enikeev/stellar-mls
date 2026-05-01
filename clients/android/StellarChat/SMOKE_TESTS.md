# Smoke-тесты Onym Android — pre-release чек

Список сценариев для **ручного** прогона перед каждым релизом. Цель — за 10–15 минут на одном устройстве удостовериться, что критический путь не сломан. Сценарии один-в-один соответствуют стадиям авто-теста `uitests.SoloUserJourneyTest` — если что-то падает в проде, проверь сначала, что делает соответствующая стадия в коде.

> **Как пользоваться:** проставлять `[x]` напротив каждого пункта по мере выполнения. Если хоть один пункт упал — **релиз стопаем**, заводим issue со ссылкой на соответствующую стадию авто-теста.

> **Solo-сессия.** Все сценарии проходят без второго пользователя: группы создаются на этом же устройстве, сообщения отправляются "в пустоту" (статус `SENT`/`FAILED` ловится визуально). Сценарии с двумя устройствами (deeplink invite, реальная доставка, knock-and-let-in) — **вне smoke**, в регрессию.

> **Разрушающие операции.** Smoke создаёт 4 временные группы с суффиксом `… QA` и удаляет их в финале (SM-15). Реальные пользовательские группы и identity не затрагиваются.

---

## 0. Подготовка окружения

**Перед каждым прогоном:**

- [ ] APK свежей сборки установлен на физическое устройство (не эмулятор)
- [ ] Зафиксированы данные сборки:
  - Версия (`versionName`): `_____`
  - VersionCode: `_____`
  - Flavor: `play` / `fdroid`
  - Variant: `debug` / `release`
- [ ] Зафиксировано устройство:
  - Модель: `_____`
  - Android: `_____` (API `_____`)
- [ ] Анимации выключены в Developer Options (Window/Transition/Animator scale = 0)
- [ ] Wi-Fi включён, есть интернет (Soroban testnet + Nostr-relays нужны для создания групп)
- [ ] Уведомления и звук отключены
- [ ] Прогоняющий: `_____`, дата: `_____`

> **НЕ нужно** делать `pm clear chat.onym.android` — smoke пишется так, чтобы не уничтожать пользовательские данные. Онбординг показывается только при первом запуске; если SM-1 не воспроизводится — сначала ставится свежий APK на чистое устройство.

---

## SM-1. Холодный старт + онбординг (страница 1)

> Стадия авто-теста: `stage_01_onboarding_sheet_visible_on_first_launch`

**Цель:** при первом запуске показывается онбординг-bottom sheet с темой шифрования.

### Шаги
- [ ] 1. Запустить приложение с launcher (или через `adb shell am start -n chat.onym.android/.MainActivity`)
- [ ] 2. Дождаться появления нижнего ModalBottomSheet (страница 1)

### Ожидаемый результат
- [ ] Виден sheet с заголовком про шифрование сообщений и метаданных (substring `metadata are encrypted`)
- [ ] Видна кнопка `Next` снизу
- [ ] Под sheet просвечивает экран Chats

---

## SM-2. Онбординг — все 4 страницы + Get Started

> Стадия авто-теста: `stage_02_walk_through_onboarding_and_dismiss`

**Цель:** все 4 страницы прокликиваются, последний `Get Started` закрывает sheet и пускает в Chats.

**Предусловие:** SM-1 пройден; sheet на странице 1.

### Шаги
- [ ] 1. Тапнуть `Next` → страница 2 (виден текст `Private by design`)
- [ ] 2. Тапнуть `Next` → страница 3 (виден текст `Truly shared ownership`)
- [ ] 3. Тапнуть `Next` → страница 4 (видны заголовок `What makes this different` и кнопка `Get Started`)
- [ ] 4. Тапнуть `Get Started` → sheet закрылся

### Ожидаемый результат
- [ ] Sheet полностью пропал (тэг `groupList.onboardingSheet` не виден)
- [ ] Активный экран — Chats; видно нижнее меню (`Contacts` / `Chats` / `Search` / `Settings`)
- [ ] Активный таб — `Chats`

> **Restore from Recovery Phrase** — намеренно не покрывается smoke'ом: тап одновременно навигирует и закрывает sheet (необратимо), а сам Restore переписывает identity. Проверять только в регрессии на отдельном устройстве.

---

## SM-3. Создать группу `Anarchy Crew QA` + первое сообщение

> Стадия авто-теста: `stage_03_create_anarchy_and_send_message`

**Цель:** "анархичный" governance создаётся on-chain, чат открывается, сообщение отправляется.

**Предусловие:** SM-2 завершён, экран Chats.

### Шаги
- [ ] 1. Тапнуть FAB `+` (правый нижний угол)
- [ ] 2. В диалоге `Add Group` тапнуть `Create`
- [ ] 3. На экране `New Group` (Step 1 of 2):
  - [ ] Ввести имя `Anarchy Crew QA`
  - [ ] Выбрать governance `Anarchy`
  - [ ] Тапнуть `Next`
- [ ] 4. На Step 2 of 2 тапнуть `Create`
- [ ] 5. Дождаться кнопки `Open` в топбаре (до 2 минут — Soroban testnet ledger close)
- [ ] 6. Тапнуть `Open` → открылся Chat
- [ ] 7. Ввести в поле `Hello anarchy` и тапнуть Send
- [ ] 8. Тапнуть Back → вернуться в Chats

### Ожидаемый результат
- [ ] На Chat экране сообщение `Hello anarchy` отрисовано
- [ ] У сообщения виден один из статусов: `Sending` / `Sent` / `Delivered` / `Tap to retry` (любой — главное, что иконка статуса появилась)
- [ ] В Chats есть строка `Anarchy Crew QA`

---

## SM-4. Создать `Direct Bob QA` (1v1)

> Стадия авто-теста: `stage_04_create_one_on_one_and_send_message`

**Цель:** 1v1-governance работает (это технически другой контракт, чем Anarchy).

### Шаги
- [ ] 1. FAB `+` → `Create`
- [ ] 2. Имя `Direct Bob QA`, governance `1v1`, Next → Create
- [ ] 3. Дождаться `Open`, открыть чат
- [ ] 4. Отправить `Hi Bob`
- [ ] 5. Back в Chats

### Ожидаемый результат
- [ ] Сообщение `Hi Bob` со статусной иконкой
- [ ] Группа `Direct Bob QA` есть в Chats

---

## SM-5. Создать `Democracy Council QA`

> Стадия авто-теста: `stage_05_create_democracy_and_send_message`

**Цель:** Democracy-governance создаётся.

### Шаги
- [ ] 1. FAB `+` → `Create`
- [ ] 2. Имя `Democracy Council QA`, governance `Democracy`, Next → Create
- [ ] 3. Дождаться `Open`, открыть чат
- [ ] 4. Отправить `Vote on motion`
- [ ] 5. Back

### Ожидаемый результат
- [ ] Сообщение со статусной иконкой
- [ ] Группа в Chats

---

## SM-6. Создать `Oligarchy Inner QA`

> Стадия авто-теста: `stage_06_create_oligarchy_and_send_message`

**Цель:** Oligarchy-governance создаётся (последний из 4 типов).

### Шаги
- [ ] 1. FAB `+` → `Create`
- [ ] 2. Имя `Oligarchy Inner QA`, governance `Oligarchy`, Next → Create
- [ ] 3. Дождаться `Open`, открыть чат
- [ ] 4. Отправить `Quorum check`
- [ ] 5. Back

### Ожидаемый результат
- [ ] Сообщение со статусной иконкой
- [ ] Группа в Chats; всего на экране **4 группы** с суффиксом `QA`

---

## SM-7. Открыть GroupInfo из чата + Back

> Стадия авто-теста: `stage_07_open_group_info_from_chat_and_back`

**Цель:** экран Group Info открывается с правильными hero-данными, Back возвращает в чат.

### Шаги
- [ ] 1. В Chats тапнуть `Anarchy Crew QA` → открылся Chat
- [ ] 2. Тапнуть на header (имя группы) → открылся Group Info
- [ ] 3. Тапнуть стрелку Back в TopAppBar
- [ ] 4. Тапнуть Back ещё раз → вернулись в Chats

### Ожидаемый результат
- [ ] На Group Info видно: имя группы, чип `End-to-end encrypted`, заголовок секции `MEMBERS`
- [ ] После двух Back'ов — в Chats; список 4 групп на месте

> Действия `Invite member` / `Leave group` / `Delete group` из этого экрана — **не покрываются** smoke (требуют второго пользователя или необратимо ломают группу).

---

## SM-8. Pin / Unpin группы через context menu

> Стадия авто-теста: `stage_08_pin_group_then_long_press_shows_unpin`

**Цель:** long-press открывает контекстное меню, Pin закрепляет (всплывает наверх + 📌 у имени), повторное открытие меню показывает Unpin.

### Шаги
- [ ] 1. В Chats long-press по `Anarchy Crew QA`
- [ ] 2. Открылся AlertDialog с заголовком `Anarchy Crew QA` и пунктами `📌 Pin` / `🗑️ Delete`
- [ ] 3. Тапнуть `Pin`
- [ ] 4. Диалог закрылся; группа всплыла в самый верх со значком 📌 рядом с именем
- [ ] 5. Long-press по `Anarchy Crew QA` ещё раз
- [ ] 6. Теперь в меню `❌ Unpin` (вместо Pin)
- [ ] 7. Тапнуть `Unpin`
- [ ] 8. Диалог закрылся, 📌 пропал, порядок групп вернулся к предыдущему

### Ожидаемый результат
- [ ] Pin/Unpin срабатывают мгновенно (не дольше 500 мс на каждый шаг)
- [ ] В Chats всё ещё 4 QA-группы на месте

---

## SM-9. Search — найти группу по имени

> Стадия авто-теста: `stage_09_search_finds_created_group_by_name`

**Цель:** глобальный поиск находит группу по части её имени.

### Шаги
- [ ] 1. Bottom-nav → таб `Search`
- [ ] 2. В поле поиска ввести `Anarchy Crew`
- [ ] 3. Дождаться появления результата

### Ожидаемый результат
- [ ] В выдаче есть строка `Anarchy Crew QA`
- [ ] Очистить поле — выдача снова пустая (плейсхолдер `Search across all your chats and messages.`)

---

## SM-10. Settings — Invite Key tab по умолчанию

> Стадия авто-теста: `stage_10_settings_lands_on_invite_tab_by_default`

**Цель:** при первом заходе в Settings активна вкладка Invite Key с QR-кодом и кнопкой `Share link`.

### Шаги
- [ ] 1. Bottom-nav → таб `Settings`
- [ ] 2. Видна сегмент-кнопка `Invite Key` / `Preferences` (выделен `Invite Key`)
- [ ] 3. На экране виден QR-код

### Ожидаемый результат
- [ ] Кнопка `Share link` отрисована и активна

---

## SM-11. Settings → Preferences: 5 секций видны

> Стадия авто-теста: `stage_11_settings_preferences_shows_all_sections`

**Цель:** на вкладке Preferences отрисованы все 5 заголовков секций.

### Шаги
- [ ] 1. В Settings переключиться на вкладку `Preferences`

### Ожидаемый результат — видны все 5 заголовков (поскролить, если нужно):
- [ ] `NETWORK`
- [ ] `PROTOCOL`
- [ ] `SECURITY`
- [ ] `ADVANCED`
- [ ] `ABOUT`

---

## SM-12. Settings sub-screens: 4 раза открыть и Back

> Стадия авто-теста: `stage_12_settings_sub_screens_open_and_back`

**Цель:** 4 sub-screen'а открываются, у каждого виден узнаваемый якорь, Back возвращает в Settings и **сохраняет вкладку Preferences** (см. фикс `rememberSaveable`).

**Предусловие:** в Settings → Preferences (SM-11).

### Шаги
- [ ] 1. Тапнуть строку `Relays` → виден заголовок `ADD RELAY` → Back → снова на Preferences (НЕ на Invite!)
- [ ] 2. Тапнуть `Blossom Servers` → виден `ADD SERVER` → Back → Preferences
- [ ] 3. Тапнуть `Stellar Contract` → виден `RELAYER (OPTIONAL)` → Back → Preferences
- [ ] 4. Тапнуть `Advanced` → виден `NOSTR IDENTITY` → Back → Preferences

### Ожидаемый результат
- [ ] После каждого Back: всё ещё активна вкладка `Preferences`, заголовок `Settings`
- [ ] Все 4 sub-screen открываются без задержек > 1 сек

> **Содержимое sub-screen'ов** (список реле, кнопки add/remove, статусы) — **вне smoke**: в demo-режиме часть гасится, реальный контент зависит от сети. В smoke проверяется только факт открытия экрана.

---

## SM-13. Recovery Phrase — Intro экран + Cancel

> Стадия авто-теста: `stage_13_settings_recovery_phrase_intro_and_cancel`

**Цель:** заходим в Recovery Phrase wizard, видим intro, отменяем БЕЗ ввода фразы и БЕЗ биометрики.

**Предусловие:** в Settings → Preferences.

### Шаги
- [ ] 1. Тапнуть строку `Backup Recovery Phrase`
- [ ] 2. Виден intro-экран с заголовком `Back up keys`
- [ ] 3. Тапнуть кнопку `Cancel` (текстовая, в навигации)

### Ожидаемый результат
- [ ] Wizard закрылся, вернулись в Settings (Preferences активна)

> **Дальше intro не идём** — следующий шаг wizard вызывает системный BiometricPrompt, который не управляется через Compose UI Test (и потенциально откроется поверх UI на твоём личном устройстве).

---

## SM-14. Bottom-nav: Contacts таб открывается

> Стадия авто-теста: `stage_14_contacts_tab_renders`

**Цель:** таб Contacts открывается без падений.

### Шаги
- [ ] 1. Bottom-nav → таб `Contacts`

### Ожидаемый результат
- [ ] Экран Contacts отрисован (тег `contacts.screen` присутствует)
- [ ] В зависимости от состояния разрешения READ_CONTACTS видно либо `Sync your phone contacts...`, либо список, либо `No Contacts`

> **Не тапаем кнопку синка контактов** — это запросит системное разрешение. Smoke только проверяет факт рендера экрана.

---

## SM-15. Возврат в Chats и удаление 4 QA-групп

> Стадии авто-теста: `stage_15_returning_to_chats_keeps_all_created_groups` + `@After cleanupCreatedGroups`

**Цель:** все 4 группы на месте после хождения по табам; финал — удалить 4 QA-группы, чтобы устройство вернулось в исходное состояние.

### Шаги
- [ ] 1. Bottom-nav → таб `Chats`
- [ ] 2. Видны все 4 группы:
  - [ ] `Anarchy Crew QA`
  - [ ] `Direct Bob QA`
  - [ ] `Democracy Council QA`
  - [ ] `Oligarchy Inner QA`
- [ ] 3. Удалить **каждую** через long-press → `🗑️ Delete` → `Delete` в подтверждении:
  - [ ] `Anarchy Crew QA` снесена
  - [ ] `Direct Bob QA` снесена
  - [ ] `Democracy Council QA` снесена
  - [ ] `Oligarchy Inner QA` снесена

### Ожидаемый результат
- [ ] В Chats нет ни одной группы с суффиксом `QA`
- [ ] Реальные пользовательские группы (если были) на месте, нетронутые

---

## Финальная проверка после смоука

- [ ] В системных настройках устройства приложение `chat.onym.android` не висит в "недавних" с краш-диалогом
- [ ] `adb logcat -d | grep -i 'fatal\|androidruntime'` — пусто или только не-наши крэши
- [ ] Удалённые QA-группы не вернулись после повторного открытия Chats

---

## Решение по релизу

- [ ] **GO** — все 15 пунктов smoke зелёные → можно публиковать
- [ ] **NO GO** — хотя бы один пункт упал → релиз заблокирован, заводится issue с привязкой к стадии авто-теста

Подпись: `_____`   Дата: `_____`   Решение: `GO` / `NO GO`

---

## Что НЕ покрывается этим smoke (намеренно)

> Если хочется проверить что-то ниже — это **регрессия**, заводи отдельный чек-лист и отдельный временной слот. По мере того, как фичи будут двигаться к проду, переноси точечно по одному.

**Вне solo-сессии (нужен второй пользователь / устройство):**
- OnboardLandingScreen (`You're invited`) — открывается только по deeplink-приглашению
- Реальный приём приглашения и присоединение к группе
- Multi-user сообщения и доставка через Nostr-relay
- Membership-операции с валидным charter (knock-and-let-in, kick, role change)

**Вне Compose UI Test (системный UI / разрешения):**
- Recovery Phrase reveal: после Intro вызывается `BiometricPrompt` — системное окно, не доступно через тест
- Restore identity happy-path — `replaceKeyManager` необратимо перетирает identity
- Запрос разрешений: Camera, Contacts, Notifications, Microphone
- QR-сканирование и `Paste from Clipboard` (Scan просит Camera, Paste — мусор из буфера)

**Деструктивные / необратимые:**
- Settings → "Generate new identity" (confirm-path) — пересоздаёт BIP39, делает все группы read-only
- Leave group / Delete group из GroupInfoScreen (есть только удаление через long-press из списка)
- F-droid-specific сценарии (если smoke запускается на play-флейворе)

**Не реализованные в коде или dead-code:**
- LegacyIdentityScreen (legacy pre-BIP39 инстанс)
- FirstGroupScreen / FirstJoinScreen (определены, но без точки входа в `MainActivity`)

**Не глубоко smoke'аем (только факт рендера):**
- Settings sub-screens — содержимое (список реле, статусы blossom, контракт): зависит от сети, в smoke только заголовок
- ContactsScreen — список контактов: зависит от системного разрешения

---

## Связанные авто-тесты

Если smoke упал — сначала прогнать соответствующую стадию авто-теста: если она зелёная, проблема скорее всего в окружении/устройстве, а не в коде.

| Smoke | Стадия `uitests.SoloUserJourneyTest` |
|---|---|
| SM-1 | `stage_01_onboarding_sheet_visible_on_first_launch` |
| SM-2 | `stage_02_walk_through_onboarding_and_dismiss` |
| SM-3 | `stage_03_create_anarchy_and_send_message` |
| SM-4 | `stage_04_create_one_on_one_and_send_message` |
| SM-5 | `stage_05_create_democracy_and_send_message` |
| SM-6 | `stage_06_create_oligarchy_and_send_message` |
| SM-7 | `stage_07_open_group_info_from_chat_and_back` |
| SM-8 | `stage_08_pin_group_then_long_press_shows_unpin` |
| SM-9 | `stage_09_search_finds_created_group_by_name` |
| SM-10 | `stage_10_settings_lands_on_invite_tab_by_default` |
| SM-11 | `stage_11_settings_preferences_shows_all_sections` |
| SM-12 | `stage_12_settings_sub_screens_open_and_back` |
| SM-13 | `stage_13_settings_recovery_phrase_intro_and_cancel` |
| SM-14 | `stage_14_contacts_tab_renders` |
| SM-15 | `stage_15_returning_to_chats_keeps_all_created_groups` + `@After cleanupCreatedGroups` |

### Прогон всего solo-journey авто-теста:

```bash
cd clients/android/StellarChat
./scripts/run-solo-journey.sh
```

Скрипт делает:
1. Прогоняет `:app:connectedPlayDebugAndroidTest` с фильтром на `uitests.SoloUserJourneyTest`.
2. После прогона `adb pull`'ит MD-отчёты с устройства в `result_autotest/autotest-reports/`.
3. На любом исходе (PASS или FAIL) отчёт включает таймлайн стадий + длительность; на FAIL добавляется stack trace + дамп logcat.

Файлы отчётов: `result_autotest/autotest-reports/solo-journey-YYYYMMDD-HHMMSS-{PASS|FAIL}.md`.

### Ручной прогон (без обёртки):

```bash
cd clients/android/StellarChat
./gradlew :app:connectedPlayDebugAndroidTest \
  -Pandroid.testInstrumentationRunnerArguments.class=uitests.SoloUserJourneyTest

# Затем достать отчёт вручную:
adb pull /sdcard/Android/data/chat.onym.android/files/autotest-reports ./result_autotest/
```
