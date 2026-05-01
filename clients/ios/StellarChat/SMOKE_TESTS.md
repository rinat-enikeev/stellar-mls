# Smoke-тесты Onym iOS — pre-release чек

Список сценариев для **ручного** прогона перед каждым релизом. Цель — за 10–15 минут на одном устройстве удостовериться, что критический путь не сломан. Сценарии соответствуют стадиям авто-теста `StellarChatUITests.SoloUserJourneyUITest` (см. таблицу в конце документа) — если что-то падает в проде, проверь сначала, что делает соответствующая стадия в коде.

> **Как пользоваться:** проставлять `[x]` напротив каждого пункта по мере выполнения. Если хоть один пункт упал — **релиз стопаем**, заводим issue со ссылкой на соответствующую стадию авто-теста.

> **Solo-сессия.** Все сценарии проходят без второго пользователя: группы создаются на этом же устройстве, сообщения отправляются "в пустоту" (статус `Sending`/`Sent`/`Delivered`/`Tap to retry` ловится визуально). Сценарии с двумя устройствами (deeplink invite, реальная доставка, knock-and-let-in) — **вне smoke**, в регрессию.

> **Разрушающие операции.** Smoke создаёт 4 временные группы с суффиксом `… QA` и удаляет их в финале (SM-15). Реальные пользовательские группы и identity не затрагиваются.

> **Порядок стадий ручного smoke vs. авто-теста.** Ручной чек-лист отсортирован по логике пользовательского сценария (создал группы → потыкал, поискал, прошёлся по настройкам). Авто-тест **выполняет SM-9 (поиск) последним**, после SM-15, не в середине: это обходной манёвр под iOS 26 — `Tab(role: .search)` после Cancel оставляет нижнюю tab-bar в свёрнутом состоянии (видно только активную «таблетку»), и XCUITest не может надёжно её раскрыть, чтобы перейти на Settings/Contacts/Chats. Для **человека** этот баг не проблема — пальцем tab-bar разворачивается естественно, поэтому ручной порядок 1→15 остаётся правильным.

> **Платформенные отличия от Android-смоук-листа:**
> - У iOS нет диалога-посредника `Add Group` — плюс в тулбаре сразу показывает меню `Create Group` / `Join Group`.
> - У iOS нет long-press контекстного меню в списке. Пин/удаление — через свайп слева (trailing edge).
> - GroupInfo на iOS открывается как `.sheet`, а не как push в навигацию — закрывается кнопкой `Close`, не системной стрелкой Back.
> - Вместо `BiometricPrompt` на Android в Recovery Phrase используется `LAContext` (Face ID / Touch ID) — XCUITest её всё равно не водит, поэтому пункт SM-13 заканчивается на `Cancel` тулбара.

---

## 0. Подготовка окружения

**Перед каждым прогоном:**

- [ ] IPA свежей сборки установлен на физическое устройство (не Simulator) — для **ручного** smoke. Авто-тест работает на симуляторе iPhone 17 Pro Max, iOS 26.0, проверено
- [ ] Зафиксированы данные сборки:
  - MARKETING_VERSION: `_____`
  - CURRENT_PROJECT_VERSION: `_____`
  - Конфигурация: `Debug` / `Release`
- [ ] Зафиксировано устройство:
  - Модель: `_____`
  - iOS: `_____`
- [ ] В Settings → Accessibility → Motion отключено `Reduce Motion`/включён по желанию (не критично, но стабильнее)
- [ ] Wi-Fi включён, есть интернет (Soroban testnet + Nostr-relays нужны для создания групп)
- [ ] Уведомления и звук отключены
- [ ] Прогоняющий: `_____`, дата: `_____`

> **НЕ нужно** удалять и переустанавливать приложение между прогонами smoke — пишется так, чтобы не уничтожать пользовательские данные. Онбординг показывается только при первом запуске; если SM-1 не воспроизводится — сначала ставится свежий IPA на чистое устройство (Settings → General → iPhone Storage → Onym → Delete App, потом установить заново).

---

## SM-1. Холодный старт + онбординг (страница 1)

> Стадия авто-теста: `stage_01_onboarding_sheet_visible_on_first_launch`

**Цель:** при первом запуске показывается онбординг-sheet с темой шифрования.

### Шаги
- [ ] 1. Запустить приложение с домашнего экрана
- [ ] 2. Дождаться появления нижнего sheet (страница 1)

### Ожидаемый результат
- [ ] Виден sheet с заголовком про шифрование сообщений и метаданных (substring `metadata are encrypted`)
- [ ] Видна кнопка `Next` снизу
- [ ] Sheet нельзя скрыть свайпом вниз (interactiveDismissDisabled)

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
- [ ] Sheet полностью пропал
- [ ] Активный экран — Chats; видна нижняя TabView (`Contacts` / `Chats` / `Search` / `Settings`)
- [ ] Активный таб — `Chats`

> **Restore from Recovery Phrase** — намеренно не покрывается smoke'ом: тап одновременно навигирует и (после успешного восстановления) закрывает sheet, переписывает identity. Проверять только в регрессии на отдельном устройстве.

---

## SM-3. Создать группу `Anarchy Crew QA` + первое сообщение

> Стадия авто-теста: `stage_03_create_anarchy_and_send_message`

**Цель:** "анархичный" governance создаётся on-chain, чат открывается, сообщение отправляется.

**Предусловие:** SM-2 завершён, экран Chats.

### Шаги
- [ ] 1. Тапнуть `+` (правый верхний угол)
- [ ] 2. В выпадающем меню выбрать `Create Group`
- [ ] 3. На экране `New Group` (Step 1 of 2):
  - [ ] Ввести имя `Anarchy Crew QA`
  - [ ] В сегмент-пикере governance выбрать `Anarchy`
  - [ ] Тапнуть `Next`
- [ ] 4. На Step 2 of 2 тапнуть `Create`
- [ ] 5. Дождаться кнопки `Open` в правом верхнем углу (до 2 минут — Soroban testnet ledger close)
- [ ] 6. Тапнуть `Open` → открылся Chat
- [ ] 7. Ввести в поле `Hello anarchy` и тапнуть кнопку отправки (стрелка вверх)
- [ ] 8. Тапнуть стрелку Back в навигации → вернуться в Chats

### Ожидаемый результат
- [ ] На Chat экране сообщение `Hello anarchy` отрисовано
- [ ] У сообщения виден один из статусов: `Sending` (часы) / `Sent` (галочка) / `Delivered` (синий чекмарк) / `Tap to retry` (красный кружок) — любой
- [ ] В Chats есть строка `Anarchy Crew QA`

---

## SM-4. Создать `Direct Bob QA` (1v1)

> Стадия авто-теста: `stage_04_create_one_on_one_and_send_message`

**Цель:** 1v1-governance работает (это технически другой контракт, чем Anarchy).

### Шаги
- [ ] 1. `+` → `Create Group`
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
- [ ] 1. `+` → `Create Group`
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
- [ ] 1. `+` → `Create Group`
- [ ] 2. Имя `Oligarchy Inner QA`, governance `Oligarchy`, Next → Create
- [ ] 3. Дождаться `Open`, открыть чат
- [ ] 4. Отправить `Quorum check`
- [ ] 5. Back

### Ожидаемый результат
- [ ] Сообщение со статусной иконкой
- [ ] Группа в Chats; всего на экране **4 группы** с суффиксом `QA`

---

## SM-7. Открыть GroupInfo из чата + Close

> Стадия авто-теста: `stage_07_open_group_info_from_chat_and_back`

**Цель:** sheet Group Info открывается с правильными hero-данными, Close возвращает в чат.

### Шаги
- [ ] 1. В Chats тапнуть `Anarchy Crew QA` → открылся Chat
- [ ] 2. Тапнуть иконку `person.3` в правом верхнем углу → открылся sheet Group Info
- [ ] 3. Тапнуть `Close` в тулбаре sheet'а
- [ ] 4. Тапнуть Back в навигации → вернулись в Chats

### Ожидаемый результат
- [ ] На Group Info видно: имя группы, чип `End-to-end encrypted`, заголовок секции `MEMBERS`
- [ ] После Close + Back — в Chats; список 4 групп на месте

> Действия `Add people` / `Leave Group` / `Rotate encryption key` из этого экрана — **не покрываются** smoke (требуют второго пользователя или необратимо ломают группу).

---

## SM-8. Pin / Unpin группы через свайп

> Стадия авто-теста: `stage_08_pin_group_then_swipe_shows_unpin`

**Цель:** trailing-свайп открывает swipe-actions, тап `Pin` закрепляет (📌 рядом с именем), повторный свайп показывает `Unpin`.

### Шаги
- [ ] 1. В Chats свайпнуть `Anarchy Crew QA` справа налево (trailing)
- [ ] 2. Появились action-кнопки: красная `Delete` и оранжевая `Pin`
- [ ] 3. Тапнуть `Pin`
- [ ] 4. Список перерисовался; рядом с именем виден оранжевый `pin.fill`
- [ ] 5. Свайпнуть `Anarchy Crew QA` ещё раз справа налево
- [ ] 6. Теперь оранжевая action-кнопка называется `Unpin` (вместо `Pin`)
- [ ] 7. Тапнуть `Unpin`
- [ ] 8. Иконка 📌 пропала, порядок групп вернулся к предыдущему

### Ожидаемый результат
- [ ] Pin/Unpin срабатывают мгновенно (не дольше 500 мс на каждый шаг)
- [ ] В Chats всё ещё 4 QA-группы на месте

---

## SM-9. Search — найти группу по имени

> Стадия авто-теста: `stage_09_search_finds_created_group_by_name`
> **В авто-тесте эта стадия выполняется последней** (после SM-15) — см. вступительную заметку про порядок стадий. В ручном smoke порядок естественный.

**Цель:** глобальный поиск находит группу по части её имени.

### Шаги
- [ ] 1. Bottom-tab → `Search`
- [ ] 2. Перед вводом — на экране плейсхолдер `Search across all your chats and messages.`
- [ ] 3. В поле поиска ввести `Anarchy Crew`
- [ ] 4. Дождаться появления результата
- [ ] 5. Тапнуть `Cancel` справа от поля поиска

### Ожидаемый результат
- [ ] В выдаче в секции `Chats` есть строка `Anarchy Crew QA`
- [ ] После Cancel — выдача очищена, плейсхолдер `Search across all your chats and messages.` снова виден
- [ ] **iOS 26 нюанс:** после Cancel нижняя tab-bar может остаться свёрнутой (видна одна «таблетка» активного таба). Это нормально — для перехода на другой таб тапни эту таблетку и tab-bar развернётся обратно

---

## SM-10. Settings — Invite Key tab по умолчанию

> Стадия авто-теста: `stage_10_settings_lands_on_invite_tab_by_default`

**Цель:** при первом заходе в Settings активна вкладка Invite Key с QR-кодом и кнопкой `Share link`.

### Шаги
- [ ] 1. Bottom-tab → `Settings`
- [ ] 2. Виден сегмент-пикер `Invite Key` / `Preferences` (выделен `Invite Key`)
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

**Цель:** 4 sub-screen'а открываются (NavigationStack push), у каждого виден ожидаемый navigation title, Back возвращает в Settings и **сохраняет вкладку Preferences**.

**Предусловие:** в Settings → Preferences (SM-11).

### Шаги
- [ ] 1. Тапнуть строку `Relays` → виден navigation title `Relays` → Back → снова на Preferences (НЕ на Invite!)
- [ ] 2. Тапнуть `Blossom Servers` → виден navigation title `Blossom Servers` → Back → Preferences
- [ ] 3. Тапнуть `Stellar Contract` → виден navigation title `Stellar Contract` → Back → Preferences
- [ ] 4. Тапнуть `Advanced` → виден navigation title `Advanced` → Back → Preferences

### Ожидаемый результат
- [ ] После каждого Back: всё ещё активна вкладка `Preferences`, navigation title `Settings`
- [ ] Все 4 sub-screen открываются без задержек > 1 сек

> **Содержимое sub-screen'ов** (список реле, кнопки add/remove, статусы) — **вне smoke**: реальный контент зависит от сети. В smoke проверяется только факт открытия экрана и navigation title.

---

## SM-13. Recovery Phrase — Intro экран + Cancel

> Стадия авто-теста: `stage_13_settings_recovery_phrase_intro_and_cancel`

**Цель:** заходим в Recovery Phrase wizard, видим intro, отменяем БЕЗ ввода фразы и БЕЗ биометрики.

**Предусловие:** в Settings → Preferences.

### Шаги
- [ ] 1. Тапнуть строку `Backup Recovery Phrase`
- [ ] 2. Виден intro-экран с navigation title `Back up keys`
- [ ] 3. Тапнуть кнопку `Cancel` (тулбар, leading)

### Ожидаемый результат
- [ ] Wizard закрылся, вернулись в Settings (Preferences активна)

> **Дальше intro не идём** — следующий шаг wizard вызывает системный `LAContext` (Face ID / Touch ID), который не управляется через XCUITest (и потенциально зачерпнёт реальные системные подтверждения на твоём личном устройстве).

---

## SM-14. Bottom-tab: Contacts таб открывается

> Стадия авто-теста: `stage_14_contacts_tab_renders`

**Цель:** таб Contacts открывается без падений.

### Шаги
- [ ] 1. Bottom-tab → `Contacts`

### Ожидаемый результат
- [ ] Экран Contacts отрисован (navigation title `Contacts`)
- [ ] В зависимости от состояния разрешения Contacts видно либо empty state, либо список, либо приглашение синка

> **Не тапаем кнопку синка контактов** — это запросит системное разрешение на доступ к контактам. Smoke только проверяет факт рендера экрана.

---

## SM-15. Возврат в Chats и удаление 4 QA-групп

> Стадии авто-теста: `stage_15_returning_to_chats_keeps_all_created_groups` + `tearDown cleanupCreatedGroups`

**Цель:** все 4 группы на месте после хождения по табам; финал — удалить 4 QA-группы, чтобы устройство вернулось в исходное состояние.

### Шаги
- [ ] 1. Bottom-tab → `Chats`
- [ ] 2. Видны все 4 группы:
  - [ ] `Anarchy Crew QA`
  - [ ] `Direct Bob QA`
  - [ ] `Democracy Council QA`
  - [ ] `Oligarchy Inner QA`
- [ ] 3. Удалить **каждую** через trailing-свайп → красная `Delete`:
  - [ ] `Anarchy Crew QA` снесена
  - [ ] `Direct Bob QA` снесена
  - [ ] `Democracy Council QA` снесена
  - [ ] `Oligarchy Inner QA` снесена

### Ожидаемый результат
- [ ] В Chats нет ни одной группы с суффиксом `QA`
- [ ] Реальные пользовательские группы (если были) на месте, нетронутые

---

## Финальная проверка после смоука

- [ ] В Recents (App Switcher) приложение `Onym` не висит с краш-баннером
- [ ] `xcrun simctl spawn booted log show --predicate 'process == "StellarChat"' --last 5m | grep -i 'crash\|fatal'` — пусто (для simulator) ИЛИ `Settings → Privacy → Analytics & Improvements → Analytics Data` не содержит свежих `StellarChat-*.ips` (для физического устройства)
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
- OnboardLandingView (`You're invited`) — открывается только по deeplink-приглашению
- Реальный приём приглашения и присоединение к группе через `DeepLinkJoinGroupView`
- Multi-user сообщения и доставка через Nostr-relay
- Membership-операции с валидным charter (knock-and-let-in, kick, role change)
- Verify on-chain (свайп leading edge → `Verify`) — требует confirmed publish

**Вне XCUITest (системный UI / разрешения):**
- Recovery Phrase reveal: после Intro вызывается `LAContext` (Face ID / Touch ID) — системное окно, не управляется тестом
- Restore identity happy-path — `replaceKeyManager` необратимо перетирает identity
- Запрос разрешений: Camera, Contacts, Notifications, Microphone, Photos
- QR-сканирование (`QRScannerView`) и `Paste invite key` (Scan просит Camera, Paste — мусор из буфера)
- Universal Link / custom URL scheme handlers (требуют `xcrun simctl openurl` и могут конфликтовать с реальными Universal Links устройства)

**Деструктивные / необратимые:**
- Settings → "Generate new identity" (alert "Generate new identity?") — пересоздаёт BIP39, делает все группы read-only
- Leave Group / любые destructive-actions из GroupInfoView (есть только удаление через trailing-swipe из списка)
- Rotate encryption key — необратимо пересоздаёт epoch

**Не реализованные в коде или dead-code:**
- LegacyIdentityScreen (legacy pre-BIP39 инстанс)
- FirstGroupView / FirstJoinView (определены, но без точки входа в `ContentView`)

**Не глубоко smoke'аем (только факт рендера):**
- Settings sub-screens — содержимое (список реле, статусы blossom, контракт): зависит от сети, в smoke только nav title
- ContactsView — список контактов: зависит от системного разрешения
- ShareLink-действия (`Share link…`) — открывают системный share sheet, не валидируем содержимое

---

## Связанные авто-тесты

Если smoke упал — сначала прогнать соответствующую стадию авто-теста: если она зелёная, проблема скорее всего в окружении/устройстве, а не в коде.

В таблице ниже **левый столбец — порядок ручного smoke** (естественный пользовательский сценарий). **Средний столбец — порядок выполнения в авто-тесте** (search вынесен в самый конец из-за iOS 26 collapsed-tab-bar quirk). Имена методов всё равно остались `stage_01`…`stage_15`, поэтому маппинг 1:1.

| Smoke | Порядок в авто-тесте | Стадия `StellarChatUITests.SoloUserJourneyUITest` |
|---|---|---|
| SM-1 | 1 | `stage_01_onboarding_sheet_visible_on_first_launch` |
| SM-2 | 2 | `stage_02_walk_through_onboarding_and_dismiss` |
| SM-3 | 3 | `stage_03_create_anarchy_and_send_message` |
| SM-4 | 4 | `stage_04_create_one_on_one_and_send_message` |
| SM-5 | 5 | `stage_05_create_democracy_and_send_message` |
| SM-6 | 6 | `stage_06_create_oligarchy_and_send_message` |
| SM-7 | 7 | `stage_07_open_group_info_from_chat_and_back` |
| SM-8 | 8 | `stage_08_pin_group_then_swipe_shows_unpin` |
| SM-9 | **15** ⚠️ | `stage_09_search_finds_created_group_by_name` |
| SM-10 | 9 | `stage_10_settings_lands_on_invite_tab_by_default` |
| SM-11 | 10 | `stage_11_settings_preferences_shows_all_sections` |
| SM-12 | 11 | `stage_12_settings_sub_screens_open_and_back` |
| SM-13 | 12 | `stage_13_settings_recovery_phrase_intro_and_cancel` |
| SM-14 | 13 | `stage_14_contacts_tab_renders` |
| SM-15 | 14 | `stage_15_returning_to_chats_keeps_all_created_groups` + `tearDown cleanupCreatedGroups` |

### Известные ограничения авто-теста (не блокируют PASS, но видны в отчёте)

- **TearDown cleanup может оставить QA-группы на устройстве.** После того как авто-тест выполнил search-стадию последней, iOS 26 удерживает нижнюю tab-bar в свёрнутом виде (`value: Collapsed`). `tearDown` пытается переключиться на вкладку Chats через `app.tabBars.buttons["Chats"]`, но та может быть недоступна → 4 QA-группы остаются. Их видно в Chats при следующем ручном запуске app — можно удалить руками trailing-swipe → Delete. Авто-тест следующего прогона создаёт **другие** QA-группы (с теми же именами — будут дубли). TODO: добавить tab-bar restoration в начало tearDown.

- **Авто-тест не проверяет `Restore from Recovery Phrase`** в SM-2 и `Backup Recovery Phrase` reveal-flow в SM-13. Оба требуют Face ID / биометрики (`LAContext`), которую XCUITest не водит. Эти ветки покрываются только ручным smoke.

- **Welcome-banner в чате** (`Your group is ready…`) появляется в каждом чате при первом входе. Авто-тест его не дисмиссит — просто игнорирует. Если в продакшене баннер начнёт перекрывать message input, авто-тест не поймает (но пользователь поймает).

### Prerequisites для прогона авто-теста

- **macOS** с Xcode 26.0+ (проверено на 26.0.1)
- **Симулятор iPhone 17 Pro Max, iOS 26.0** (загружается через Xcode → Settings → Components)
- **`xcodegen`** (`brew install xcodegen`) — генерирует `.xcodeproj` из `project.yml`
- **Rust toolchain** (`brew install rustup` → `rustup-init -y`) с таргетом `aarch64-apple-ios-sim` — нужен **один раз** при первой сборке для компиляции `SEPMLSFFI.xcframework` (FFI-binding к Rust-крейту `sep-xxxx-circuits`)
- **Интернет**: реальный Soroban testnet RPC + Nostr-relays используются в стадиях SM-3 / SM-4 / SM-5 / SM-6 (создание групп on-chain). Без интернета эти стадии валятся по таймауту 120 сек

### Прогон всего solo-journey авто-теста:

```bash
cd "clients/ios/StellarChat"
./scripts/run-solo-journey.sh
```

Что делает скрипт:
1. Регенерирует `StellarChat.xcodeproj` через `xcodegen` (если `project.yml` свежее).
2. Прогоняет `xcodebuild test` с фильтром на `StellarChatUITests/SoloUserJourneyUITest`.
3. Из xcodebuild-лога вытаскивает Markdown-отчёт между маркерами `=== MARKDOWN REPORT BEGIN/END ===` и сохраняет его в `result_autotest/`.
4. На любом исходе (PASS или FAIL) отчёт включает таймлайн стадий с offset'ами + длительность; на FAIL добавляется last-stage + текст ошибки.

Файлы отчётов: `result_autotest/solo-journey-YYYYMMDD-HHMMSS-{PASS|FAIL}.md`.
**Время прогона:** ~5 минут на чистом симуляторе (build ~60s + 4×Soroban create ~150s + остальные стадии ~90s + cleanup).

### Первая сборка (только при первом запуске на новой машине):

```bash
# 1. Поставить инструменты
brew install xcodegen rustup
rustup-init -y --no-modify-path --default-toolchain none --profile minimal
export PATH="$HOME/.cargo/bin:$PATH"

# 2. Собрать xcframework для симулятора (~40s)
cd "/path/to/repo"
cargo build --manifest-path Cargo.toml --release --target aarch64-apple-ios-sim
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios-sim/release/libsep_xxxx_circuits.a \
  -headers swift-mls/Sources/CSEPMLSFFI/include \
  -output build/SEPMLSFFI.xcframework

# 3. Прогон
cd clients/ios/StellarChat
./scripts/run-solo-journey.sh
```

### Ручной прогон (без обёртки):

```bash
cd "clients/ios/StellarChat"
xcodegen generate    # если project.yml менялся
xcodebuild test \
  -scheme StellarChat \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro Max' \
  -only-testing:StellarChatUITests/SoloUserJourneyUITest \
  CODE_SIGNING_REQUIRED=NO CODE_SIGNING_ALLOWED=NO CODE_SIGN_IDENTITY=""

# Затем достать отчёт из xcresult вручную:
xcrun xcresulttool get --path TestResults.xcresult --format json
# (XCAttachment с именем solo-journey-*.md лежит внутри)
```
