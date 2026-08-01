<p align="center">
  <img src="assets/logo.svg" width="116" alt="Логотип monk" />
</p>

<h1 align="center">monk</h1>

<p align="center">
  <b>фокус без компромиссов.</b><br/>
  Кроссплатформенный CLI-блокировщик отвлечений на Rust.<br/>
  Один бинарник, один демон, без лишнего – блокируй приложения и сайты,<br/>
  запускай жёсткие сессии и возвращай себе внимание.
</p>

<p align="center">
  <a href="https://github.com/mdportnov/monk-cli/actions/workflows/ci.yml"><img src="https://github.com/mdportnov/monk-cli/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/mdportnov/monk-cli/actions/workflows/release.yml"><img src="https://github.com/mdportnov/monk-cli/actions/workflows/release.yml/badge.svg" alt="Release" /></a>
  <img src="https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue.svg" alt="Лицензия: MIT OR Apache-2.0" />
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-5a5a5a" alt="Платформы" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-1.82%2B-DEA584?logo=rust&logoColor=black" alt="Rust" />
  <img src="https://img.shields.io/badge/TUI-ratatui-7aa2f7" alt="ratatui" />
  <img src="https://img.shields.io/badge/async-tokio-orange" alt="tokio" />
  <img src="https://img.shields.io/badge/stats-SQLite-044a64?logo=sqlite&logoColor=white" alt="SQLite" />
  <img src="https://img.shields.io/badge/unsafe-forbidden-success" alt="deny(unsafe_code)" />
  <img src="https://img.shields.io/badge/i18n-EN%20%2F%20RU-bb9af7" alt="EN/RU" />
</p>

<p align="center">
  🇬🇧 English version: <a href="./README.md">README.md</a>
</p>

---

## Ключевые возможности

- **Честная блокировка приложений** — сканирует установленные программы на macOS, Linux (нативные, Flatpak и Snap) и Windows, ты выбираешь из реального списка, а не угадываешь имена процессов.
- **Готовые наборы сайтов** — встроенные группы `global` и `ru` (соцсети, видео, новости, мессенджеры, шопинг, игры) с автоматическим раскрытием поддоменов.
- **Hard mode** — подписанный BLAKE3 keyed-HMAC замок сессии, устойчивый к подделке. Ни `monk stop`, ни правка конфига, ни убийство демона не помогут.
- **Фоновой демон** — IPC через Unix-сокет или named pipe, fail-closed цикл реконциляции, корректная очистка при SIGTERM, установка как systemd / launchd / планировщик задач Windows.
- **Интерактивный TUI** — дашборд на ratatui для сессий, живой статистики, редактирования профилей, поиска режимов по вводу, прогресса дневного лимита и заметного бейджа hard mode.
- **Локализация** — английский и русский из коробки через `rust-i18n`.
- **Zero unsafe** — `#![deny(unsafe_code)]` во всём основном крейте.

## Как это работает

monk запускает небольшой постоянный демон — это тот же бинарник `monk`, запущенный как `monk daemon run` и зарегистрированный в менеджере сервисов ОС под именем `monkd`. Он держит состояние блокировок; CLI и TUI общаются с ним по локальному сокету. При старте сессии:

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

### Быстрая установка (скрипт)

Скачивает подходящий релизный бинарник, проверяет контрольную сумму и кладёт
его в `PATH`. После этого запусти `monk setup`.

```sh
# Linux / macOS  → ставит в ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/mdportnov/monk-cli/master/assets/install.sh | bash
```

```powershell
# Windows (PowerShell 5+)  → ставит в %LOCALAPPDATA%\monk\bin
irm https://raw.githubusercontent.com/mdportnov/monk-cli/master/assets/install.ps1 | iex
```

### Из исходников

Нужен Rust toolchain (1.82+) — поставь через [rustup](https://rustup.rs). Одинаково работает на Linux, macOS и Windows.

```sh
git clone https://github.com/mdportnov/monk-cli
cd monk-cli

# Вариант A — собрать release-бинарник и положить в PATH (рекомендуется)
cargo install --path .     # → ~/.cargo/bin/monk  (%USERPROFILE%\.cargo\bin\monk.exe на Windows)

# Вариант B — собрать release-бинарник в дереве и запускать по пути
cargo build --release      # → target/release/monk  (target\release\monk.exe на Windows)
```

Обычный `cargo build` (без `--release`) даёт медленный debug-бинарник в `target/debug/monk` — только для разработки.

#### Скрипт установки в один заход

Не хочешь делать шаги вручную? Из клона репозитория этот скрипт проверит наличие
Rust toolchain (и предложит поставить его), соберёт и установит `monk` в `PATH`
и в конце запустит `monk setup` — комментируя каждый шаг.

**Пререквизиты:** [git](https://git-scm.com/downloads) чтобы клонировать репозиторий, плюс
C-линковщик, нужный сборке Rust — [Xcode Command Line Tools](https://developer.apple.com/xcode/resources/)
на macOS (`xcode-select --install`), `build-essential` на Linux или
[Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
на Windows. Сам [Rust toolchain](https://rustup.rs) (1.82+) скрипт поставит, если его нет.

```sh
# Linux / macOS
git clone https://github.com/mdportnov/monk-cli && cd monk-cli
./scripts/setup.sh
```

```powershell
# Windows (PowerShell 5+) — открой терминал от администратора для привилегированного шага
git clone https://github.com/mdportnov/monk-cli; cd monk-cli
powershell -ExecutionPolicy Bypass -File .\scripts\setup.ps1
```

### cargo-binstall

```sh
cargo binstall monk
```

### Пакеты

- **Debian / Ubuntu**: `cargo deb` собирает `.deb` с systemd user unit; автодополнения bash/zsh/fish включены.
- **Fedora / RHEL**: `cargo generate-rpm` собирает `.rpm` (автодополнения включены).
- **Windows**: `assets/install.ps1` (выше). MSI / Scoop — скоро.
- **macOS**: Homebrew tap — скоро.

### Требования

- Терминал и права администратора на машине — блокировка правит системный файл `hosts`, а это привилегированная операция. Доступ выдаётся **один раз**, при установке.
- Rust 1.82+ — только если собираешь из исходников.

`monk setup` сам настраивает права; под капотом это работает по-разному в зависимости от ОС:

- **macOS** — демон ставится как системный сервис, работающий от **root**, и владеет `/etc/hosts`. При установке появляется нативный запрос пароля macOS; эквивалент в CLI — `sudo monk service install`.
- **Linux** — демон работает от **тебя** через *пользовательский* unit `systemd` (без `sudo`). Чтобы блокировка работала, monk должен иметь право писать в `/etc/hosts` (или использовать бэкенд `systemd-resolved`) — `monk doctor` подскажет, если не может.
- **Windows** — демон это задача планировщика при входе, которой нужны повышенные права, поэтому открой терминал **от администратора** перед `monk setup` / `monk daemon install`.

## Быстрый старт

Выполняй по порядку. На **Windows** сначала открой PowerShell **от администратора**; на **macOS** при установке появится запрос пароля.

```sh
monk setup                  # 1. визард первого запуска: язык, профиль, демон, автодополнения, проверка
monk doctor                 # 2. убедиться, что всё на месте (демон запущен, hosts доступен на запись)
monk start deepwork -d 50m  # 3. запустить сессию фокуса на 50 минут
monk status                 # 4. что заблокировано и сколько осталось
monk stop                   # 5. завершить — только soft mode; hard mode остановить нельзя
```

Нужен интерфейс? `monk tui` открывает полный дашборд. Запустить сессию без возможности выйти — `monk start deepwork --hard`.

## Команды

### Сессии

| Команда                                               | Что делает                                  |
| ----------------------------------------------------- | ------------------------------------------- |
| `monk start [PROFILE] [-d DUR] [--hard] [--reason …]` | Запустить сессию фокуса (`--hard` = без отмены) |
| `monk stop`                                           | Завершить активную сессию (только soft mode) |
| `monk panic [--phrase …] [--cancel]`                  | Запросить — или отменить — отложенный выход из hard mode |
| `monk status`                                         | Статус демона и сессии                      |
| `monk stats`                                          | Статистика сессий                           |
| `monk tui`                                            | Открыть интерактивный дашборд               |

### Профили и приложения

| Команда                                                        | Что делает                                |
| -------------------------------------------------------------- | ----------------------------------------- |
| `monk profiles`                                                | Список профилей                           |
| `monk profile show NAME [--json]`                              | Показать полный конфиг профиля            |
| `monk profile create NAME [--preset P]`                        | Создать пустой профиль или из пресета     |
| `monk profile duplicate SOURCE [TARGET]`                       | Скопировать профиль под новым именем      |
| `monk profile edit NAME`                                       | Интерактивное редактирование              |
| `monk profile edit NAME --add/--remove ID`                     | Правки для скриптов                       |
| `monk profile limits NAME [--max/--min/--cooldown/--daily-cap] [--clear]` | Задать или сбросить лимиты времени |
| `monk profile delete NAME`                                     | Удалить профиль                           |
| `monk apps list [--refresh]`                                   | Показать кэш установленных приложений     |
| `monk apps scan`                                               | Принудительное пересканирование          |

Встроенные пресеты для `--preset`: `deepwork`, `study`, `detox`, `sleep`, `sober`, `lockdown`, `no-social`, `no-video`, `no-news`, `no-games`, `no-chat`, `no-shopping`, `no-adult`, `no-gambling`, `no-dating`, `no-ai`.

### Демон

`monk service` — алиас для `monk daemon`. Команде `install` нужны повышенные права: `sudo monk service install` на macOS (или запрос пароля при установке), терминал от администратора на Windows; на Linux это пользовательский unit `systemd` и `sudo` не требуется.

| Команда                             | Что делает                                          |
| ----------------------------------- | --------------------------------------------------- |
| `monk daemon start`                 | Запустить фоновый демон                             |
| `monk daemon stop`                  | Корректно остановить                                |
| `monk daemon status`                | То же, что `monk status`                            |
| `monk daemon run`                   | Запуск на переднем плане (используется менеджером сервисов; вручную обычно не нужен) |
| `monk daemon install [--reinstall]` | Установить как systemd / launchd / задачу планировщика Windows |
| `monk daemon uninstall [--purge]`   | Удалить сервис (`--purge` также стирает конфиг и данные) |

### Конфиг и диагностика

| Команда                                | Что делает                                          |
| -------------------------------------- | --------------------------------------------------- |
| `monk setup` / `monk init [--quick] [--reset] [-y]` | Визард первого запуска: конфиг, демон, автодополнения, doctor |
| `monk doctor [--json] [--fix]`         | Проверка окружения, прав и здоровья демона; `--fix` чинит частые проблемы |
| `monk config path`                     | Показать путь к файлу конфига                       |
| `monk config export`                   | Выгрузить текущий конфиг                            |
| `monk config import FILE`              | Проверить и импортировать конфиг                    |
| `monk lang en\|ru`                     | Сменить язык интерфейса                             |
| `monk completions SHELL`               | Сгенерировать автодополнение (bash/zsh/fish/powershell/elvish) |

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

Двойная лицензия: [MIT](LICENSE-MIT) или [Apache-2.0](LICENSE-APACHE) на выбор.

---

<p align="center">
  Автор – <a href="https://mikeportnov.com/ru/projects">Mike Portnov</a> · <a href="https://github.com/mdportnov">@mdportnov</a>
</p>
