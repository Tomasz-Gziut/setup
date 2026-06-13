# setup

```powershell
cargo clean
cargo install --path . --force
setup
```

CLI/TUI do zarzadzania aplikacjami Windows przez `winget`.

Domyslne uruchomienie komendy `setup` otwiera interaktywny widok TUI do tworzenia presetow aplikacji. Binarka zawiera manifest Windows `asInvoker`, zeby sama nazwa `setup.exe` nie uruchamiala automatycznie okna UAC.

## Wymagania

- Windows 10/11
- Rust/Cargo
- `winget` (Windows Package Manager)

## Instalacja

```powershell
cargo clean
cargo install --path . --force
setup
```

Po instalacji `setup.exe` trafia zwykle do:

```powershell
C:\Users\<user>\.cargo\bin\setup.exe
```

Sprawdz, ktora binarka jest uruchamiana:

```powershell
where.exe setup
```

## Uzycie

### Interaktywny manager

```powershell
setup
```

Uruchamia TUI, w ktorym mozna wyszukiwac aplikacje, zaznaczac je, zapisywac presety oraz instalowac albo odinstalowywac wybrane pozycje.

### Pokaz zainstalowane aplikacje

```powershell
setup show
```

Wyswietla zainstalowane aplikacje w tabeli: `Name`, `ID`, `Version`, `Source`.

### Eksportuj konfiguracje

```powershell
setup export .\my-apps.json
```

Eksportuje liste zainstalowanych aplikacji do pliku JSON. Podczas eksportu narzedzie:

- wyklucza aplikacje systemowe, sterowniki i runtime'y,
- sprawdza dostepnosc aplikacji w `winget`,
- oznacza niedostepne aplikacje jako `availableInWinget: false`.

### Instaluj z konfiguracji

```powershell
setup install .\my-apps.json
```

Instaluje aplikacje z pliku konfiguracyjnego przez `winget`. Aplikacje oznaczone jako niedostepne sa pomijane.

Instalacja i deinstalacja uzywaja `winget --silent --disable-interactivity`, zeby ograniczyc osobne okna instalatorow.

## Format config.json

```json
{
  "apps": [
    {
      "id": "Microsoft.VisualStudioCode",
      "name": "Visual Studio Code",
      "version": "latest",
      "availableInWinget": true
    },
    {
      "id": "SomeApp",
      "name": "Some App",
      "version": "latest",
      "availableInWinget": false,
      "note": "Not available via winget - install manually"
    }
  ]
}
```

## Build

Debug build:

```powershell
cargo build
```

Release/installowana binarka:

```powershell
cargo install --path . --force
```

Projekt osadza manifest Windows przez `build.rs` i `app.manifest`, z poziomem uprawnien `asInvoker`.
