# setup

CLI tool do zarządzania aplikacjami Windows przez winget. Standalone EXE - działa na świeżym Windowsie bez żadnych zależności.

## Użycie

### Pokaż zainstalowane aplikacje
```bash
setup.exe show
```
Wyświetla wszystkie zainstalowane aplikacje w tabeli (Name, ID, Version, Source).

### Eksportuj config
```bash
setup.exe export ./my-apps.json
```
Eksportuje listę zainstalowanych aplikacji do pliku JSON. Automatycznie:
- Wyklucza aplikacje systemowe, sterowniki i runtimey
- Sprawdza dostępność każdej aplikacji w winget
- Oznacza niedostępne aplikacje flagą `availableInWinget: false`

### Zainstaluj z configa
```bash
setup.exe install ./my-apps.json
```
Instaluje wszystkie aplikacje z pliku config przez winget. Aplikacje oznaczone jako niedostępne są pomijane z odpowiednim komunikatem.

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
      "note": "Niedostępne przez winget - zainstaluj ręcznie"
    }
  ]
}
```

## Build

```bash
npm install
npm run build:exe
```

Generuje `setup.exe` (~38 MB) - standalone executable bez potrzeby Node.js.

## Wymagania

- Windows 10/11
- winget (Windows Package Manager) - wbudowany w Windows 11, dla Windows 10 dostępny przez Microsoft Store
