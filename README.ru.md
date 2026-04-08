# monk

Кроссплатформенный блокировщик отвлечений на Rust. Один бинарник, один демон, без лишнего — блокируй приложения и сайты, запускай жёсткие сессии и возвращай себе внимание.

> 🇬🇧 English version: [README.md](./README.md)

```
  ┏┳┓ ┏┓ ┏┓ ┃┏
  ┃┃┃ ┃┃ ┃┃ ┣┻┓
  ┛ ┗ ┗┛ ┛┗ ┛ ┗
  фокус без компромиссов.
```

## Ключевые возможности

- **Честная блокировка приложений** — сканирует установленные программы на macOS, Linux и Windows, ты выбираешь из реального списка, а не угадываешь имена процессов.
- **Готовые наборы сайтов** — встроенные группы `global` и `ru` (соцсети, видео, новости, мессенджеры, шопинг, игры) с автоматическим раскрытием поддоменов.
- **Hard mode** — подписанный BLAKE3 keyed-HMAC замок сессии, устойчивый к подделке. Ни `monk stop`, ни правка конфига, ни убийство демона не помогут.
- **Фоновой демон** — IPC через Unix-сокет или named pipe, fail-closed цикл реконциляции, корректная очистка при SIGTERM, установка как systemd / launchd / Windows Service.
- **Интерактивный TUI** — дашборд на ratatui для сессий, статистики и редактирования профилей.
- **Локализация** — английский и русский из коробки через `rust-i18n`.
- **Zero unsafe** — `#![deny(unsafe_code)]` во всём основном крейте.

## Как это работает

monk запускает небольшой постоянный демон (`monkd`), который держит состояние блокировок. CLI и TUI общаются с ним по локальному сокету. При старте сессии:

1. Выбранный профиль разворачивается в конкретный список хостов и приложений.
2. Хосты записываются в системный `hosts` (атомарная запись, подписанный блок).
3. Совпавшие процессы убиваются в тик-цикле и держатся закрытыми до конца сессии.
4. В hard mode подписанный lock-файл проверяется на каждом тике — повреждение или удаление не снимают блок.

## Технологии

| Слой          | Крейты / технологии                                                   |
| ------------- | --------------------------------------------------------------------- |
| CLI           | `clap` v4 derive, `clap_complete`, `inquire` для интерактивных подсказок |
| TUI           | `ratatui`, `crossterm`, `tui-big-text`, `tachyonfx`                   |
| Async runtime | `tokio` multi-thread, `tokio-util`, `futures`                         |
| IPC           | `interprocess` (Unix domain socket / Windows named pipe)              |
| Хранение      | `toml` конфиг, `rusqlite` (bundled) для статистики, атомарная запись `fs-err` |
| Целостность   | `blake3` keyed HMAC, канонический бинарный сериализатор, `machine-uid` |
| Процессы      | `sysinfo`, `nix` сигналы на Unix, крейт `windows` на Windows          |
| Поиск приложений | `plist` (macOS bundles), парсер `.desktop` (Linux), `lnk` (Windows) |
| Наблюдаемость | `tracing`, `tracing-subscriber`, `tracing-appender`                   |
| i18n          | `rust-i18n`, `sys-locale`                                             |
| Ошибки        | `thiserror` + `miette` с красивыми репортами                          |

## Установка

### Из исходников

```sh
git clone https://github.com/mihail/monk
cd monk
cargo install --path .
```

### cargo-binstall

```sh
cargo binstall monk
```

### Пакеты

- **Debian / Ubuntu**: `cargo deb` собирает `.deb` с systemd user unit.
- **Fedora / RHEL**: `cargo generate-rpm` собирает `.rpm`.
- **macOS**: Homebrew tap — скоро.
- **Windows**: MSI / Scoop — скоро.

### Требования

- Rust 1.82+ (только для сборки из исходников)
- Один раз root / admin — чтобы monk мог писать в `hosts`
- Linux: `systemd` (user session) для `monk daemon install`
- Windows: ничего — демон регистрируется как пользовательский сервис

## Быстрый старт

```sh
monk init                       # интерактивный визард первого запуска
monk start deepwork -d 50m      # сессия на 50 минут
monk start deepwork --hard      # hard mode — отменить нельзя
monk status                     # что запущено и сколько осталось
monk stop                       # завершить сессию (только soft mode)
monk tui                        # полный дашборд
```

## Команды

### Сессии

| Команда                         | Что делает                                 |
| ------------------------------- | ------------------------------------------ |
| `monk start [profile] [-d DUR]` | Запустить сессию фокуса                    |
| `monk start … --hard`           | Запустить hard-mode сессию (без отмены)    |
| `monk stop`                     | Завершить активную сессию                  |
| `monk panic [--phrase …]`       | Запросить отложенный выход из hard mode    |
| `monk status`                   | Статус демона и сессии                     |

### Профили и приложения

| Команда                                    | Что делает                                 |
| ------------------------------------------ | ------------------------------------------ |
| `monk profiles`                            | Список профилей                            |
| `monk profile create NAME`                 | Создать пустой профиль                     |
| `monk profile delete NAME`                 | Удалить профиль                            |
| `monk profile edit NAME`                   | Интерактивное редактирование               |
| `monk profile edit NAME --add/--remove ID` | Правки для скриптов                        |
| `monk apps list [--refresh]`               | Показать кэш установленных приложений      |
| `monk apps scan`                           | Принудительное пересканирование            |

### Демон

| Команда                 | Что делает                                        |
| ----------------------- | ------------------------------------------------- |
| `monk daemon start`     | Запустить фоновый демон                           |
| `monk daemon stop`      | Корректно остановить                              |
| `monk daemon status`    | То же, что `monk status`                          |
| `monk daemon install`   | Установить как systemd / launchd / Windows Service |
| `monk daemon uninstall` | Удалить сервис                                    |

### Конфиг и диагностика

| Команда                | Что делает                                         |
| ---------------------- | -------------------------------------------------- |
| `monk doctor`          | Проверка окружения, прав и здоровья демона         |
| `monk config path`     | Показать путь к файлу конфига                      |
| `monk config export`   | Выгрузить текущий конфиг                           |
| `monk config import F` | Проверить и импортировать конфиг                   |
| `monk lang en\|ru`     | Сменить язык интерфейса                            |
| `monk completions SH`  | Сгенерировать автодополнение (bash/zsh/fish/ps)    |

## Конфигурация

Расположение:

- Linux: `~/.config/monk/config.toml`
- macOS: `~/Library/Application Support/monk/config.toml`
- Windows: `%APPDATA%\monk\config.toml`

```toml
[general]
default_profile = "deepwork"
default_duration = "50m"
hard_mode = false
autostart = true
locale = "ru"

[profiles.deepwork]
site_groups = ["global.social", "global.video", "global.news", "ru.social", "ru.news"]
sites = ["example.com"]
apps  = ["com.tinyspeck.slackmacgap", "com.hnc.Discord"]
```

Идентификаторы приложений — стабильные ключи, которые выдаёт сканер: bundle id на macOS, `.desktop` id на Linux, путь к цели ярлыка на Windows.

## Hard mode

Hard mode — главная фишка. После запуска жёсткой сессии:

- CLI отказывается выполнять `monk stop`.
- Демон игнорирует SIGTERM/SIGINT до конца сессии.
- Lock-файл подписан ключом, привязанным к стабильной идентичности машины; любая подделка детектируется и трактуется как активная блокировка.
- `monk panic` ставит отложенный выход (настраиваемая задержка), чтобы можно было отменить ошибочно запущенную сессию, но без мгновенного побега.

Пользуйся осознанно.

## Разработка

```sh
just fmt        # rustfmt
just lint       # clippy -D warnings
just test       # cargo test
just run init   # cargo run -- init
```

В репозитории включены `unsafe_code = "deny"` и строгий профиль clippy. CI гоняется на Linux, macOS и Windows.

## Лицензия

Dual-licensed: MIT или Apache-2.0.
