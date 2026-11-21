<div align="center">

<img src="assets/icon_256.png" alt="trueVK" width="150" />

<h1>trueVK</h1>

<p><strong>Музыка ВКонтакте без браузера.<br />Без Electron, Tauri и webview. Просто Rust и окно.</strong></p>

<p>
  <a href="https://github.com/bledniyxvi/trueVK/releases"><img src="https://img.shields.io/github/v/release/bledniyxvi/trueVK?display_name=tag&amp;style=for-the-badge" alt="Release" /></a>
  <a href="https://github.com/bledniyxvi/trueVK/releases"><img src="https://img.shields.io/github/downloads/bledniyxvi/trueVK/total?style=for-the-badge" alt="Downloads" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%2F11-0078D4?style=for-the-badge&amp;logo=windows" alt="Windows 10/11" />
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?style=for-the-badge&amp;logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/no%20Electron-no%20webview-brightgreen?style=for-the-badge" alt="No Electron" />
</p>

<p>
  <a href="https://github.com/bledniyxvi/trueVK/releases/latest/download/trueVK-Setup-0.1.0.exe"><img src="https://img.shields.io/badge/Download-EXE-2ea44f?style=for-the-badge&amp;logo=github" alt="Скачать установщик" /></a>
</p>

</div>

## Зачем это не браузер

| | trueVK | Вкладка браузера |
|---|---|---|
| Запуск | сразу, одно окно | браузер + вкладка + расширения |
| Память | аудио живёт в RAM | целый webview и процесс вкладки |
| Файлы на диске | кэша музыки нет | кэш, куки, профиль |
| Управление | медиаклавиши, трей, горячие клавиши | только вкладка |
| Установщик | 6,5 МБ, без прав администратора | — |

## Что умеет

<table>
<tr>
<td width="50%" valign="top">

**Музыка**

- Вкладки: «Моя музыка», плейлисты, поиск и рекомендации.
- Добавление и удаление треков: кнопки `+` и `×`, удаление с подтверждением.
- Очередь привязана к треку, а не к номеру строки: переключение вкладок и новый поиск её не сбивают.
- Prev/Next, автопродолжение, громкость и перемотка.

</td>
<td width="50%" valign="top">

**Опыт**

- Медиаклавиши и горячие клавиши `Space`, `Ctrl+←/→`; в поле ввода они не мешают.
- Закрытие окна сворачивает программу в трей, выход из трея завершает её полностью.
- Запоминаются громкость и последний воспроизведённый трек.
- Аудио потоково играет из оперативной памяти: треки не скачиваются на диск.
- Токен VK и User-Agent лежат в Windows Credential Manager, а не в файлах настроек.

</td>
</tr>
</table>

### Горячие клавиши

| Клавиша | Действие |
|---|---|
| <kbd>Space</kbd> | пауза / продолжить |
| <kbd>Ctrl</kbd> + <kbd>←</kbd> / <kbd>→</kbd> | предыдущий / следующий трек |
| Медиаклавиши | работают как в обычном плеере |

## Как выглядит

<table>
<tr>
<td width="50%" valign="top"><img src="docs/screenshots/01.jpg" alt="Скриншот 1" width="100%" /></td>
<td width="50%" valign="top"><img src="docs/screenshots/02.jpg" alt="Скриншот 2" width="100%" /></td>
</tr>
</table>

<p align="center">Скриншот 1 · Скриншот 2</p>

## Установка

1. [Скачать `trueVK-Setup-0.1.0.exe`](https://github.com/bledniyxvi/trueVK/releases/latest/download/trueVK-Setup-0.1.0.exe) или зайти в [Releases](https://github.com/bledniyxvi/trueVK/releases).
2. Запустить установщик — права администратора не нужны.
3. По умолчанию программа ставится в `%LOCALAPPDATA%\Programs\trueVK`. Ярлык в меню «Пуск» создаётся всегда, ярлык на рабочем столе — по желанию.

<details>
<summary>SHA256 установщика</summary>

`FE11824C19C438167049E6DB0D911D53228A43275EAF53562D470A601DA62735`

</details>

## Первый запуск

При первом запуске введите токен доступа VK и User-Agent в окне программы. Они сохранятся в Windows Credential Manager и больше не будут запрашиваться. Для командной строки предусмотрены переменные окружения `VK_TOKEN` и `VK_UA`.

Токен не записывается в файлы настроек и не попадает в лог. В `%APPDATA%\vkmusic\settings.json` хранятся только настройки.

## Сборка из исходников

Нужны Rust с поддержкой edition 2024 и установленный Microsoft C++ Build Tools.

```powershell
git clone https://github.com/bledniyxvi/trueVK.git
cd trueVK
cargo build --release
```

Готовый исполняемый файл: `target\release\truevk.exe`.

Чтобы собрать установщик, нужен Inno Setup 7:

```powershell
powershell -ExecutionPolicy Bypass -File installer\build.ps1
```

Скрипт сам соберёт Release, создаст `installer\Output\trueVK-Setup-0.1.0.exe` и выведет его размер и SHA256.

<details>
<summary>Структура проекта</summary>

- `src/main.rs` — точка входа и служебные режимы.
- `src/api.rs` — клиент VK API.
- `src/player.rs` — загрузка и воспроизведение из памяти.
- `src/gui.rs` — интерфейс, очередь, горячие клавиши и трей.
- `src/storage.rs` — токен в Credential Manager и локальные настройки.
- `src/theme.rs` — цвета и оформление.
- `assets/` — иконки и шрифты.
- `installer/` — скрипт сборки установщика и файл проекта Inno Setup.

</details>

## Лицензия

Лицензия на код не указана. Шрифты в `assets/fonts` распространяются по SIL Open Font License; тексты лицензий лежат рядом с файлами шрифтов.
