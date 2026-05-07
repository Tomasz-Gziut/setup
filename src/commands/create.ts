import Table from 'cli-table3';
import chalk from 'chalk';
import * as fs from 'fs';
import * as path from 'path';
import * as readline from 'readline';
import { WingetService } from '../services/winget';
import { AppConfig, Config, InstalledApp } from '../types/config';
import { PRESET_DIR } from '../constants';

const PAGE_SIZE = 10;

type InstalledMatch = {
  version: string;
  source: string;
};

type ActivePanel = 'available' | 'selected';

type PendingAction = {
  message: string;
  run: () => Promise<void> | void;
};

export async function createCommand(fileName: string): Promise<void> {
  const winget = new WingetService();
  const selectedApps: AppConfig[] = [];
  let installedApps: InstalledApp[] = [];
  let allApps: InstalledApp[] = [];
  let filteredApps: InstalledApp[] = [];
  let installedMap: Map<string, InstalledMatch> = new Map();
  let installedByName: Map<string, InstalledMatch> = new Map();
  let installedNameEntries: Array<{ normalizedName: string; match: InstalledMatch }> = [];
  const searchCache: Map<string, InstalledApp[]> = new Map();
  let filterSearchTimer: NodeJS.Timeout | undefined;
  let filterSearchToken = 0;
  let filterSearching = false;
  let activePanel: ActivePanel = 'available';
  let cursorIndex = 0;
  let selectedCursorIndex = 0;
  let filterText = '';
  let showInstalledOnly = false;
  let pendingAction: PendingAction | undefined;

  const configPath = fileName.endsWith('.json') ? fileName : `${fileName}.json`;
  const fullPath = path.join(PRESET_DIR, configPath);

  if (!fs.existsSync(PRESET_DIR)) {
    fs.mkdirSync(PRESET_DIR, { recursive: true });
  }

  const normalizeAppName = (name: string): string =>
    name
      .toLowerCase()
      .replace(/\b(x64|x86|win64|win32|stable|browser)\b/g, ' ')
      .replace(/[^a-z0-9]+/g, ' ')
      .replace(/\s+/g, ' ')
      .trim();

  const rebuildInstalledIndexes = () => {
    const nextInstalledMap = new Map<string, InstalledMatch>();
    const nextInstalledByName = new Map<string, InstalledMatch>();
    const nextInstalledNameEntries: Array<{ normalizedName: string; match: InstalledMatch }> = [];

    installedApps.forEach(app => {
      const installedMatch = {
        version: app.version,
        source: app.source || 'local/other'
      };

      if (app.id) nextInstalledMap.set(app.id.toLowerCase(), installedMatch);
      if (app.name) {
        const normalizedName = normalizeAppName(app.name);
        if (normalizedName) {
          nextInstalledByName.set(normalizedName, installedMatch);
          nextInstalledNameEntries.push({ normalizedName, match: installedMatch });
        }
      }
    });

    installedMap = nextInstalledMap;
    installedByName = nextInstalledByName;
    installedNameEntries = nextInstalledNameEntries;
  };

  console.log(chalk.cyan(`\nCreating config: ${chalk.bold(fullPath)}`));

  process.stdout.write(chalk.gray('Loading installed applications... '));
  installedApps = winget.getInstalledApps().filter(app => !winget.isSystemApp(app));
  rebuildInstalledIndexes();
  console.log(chalk.green(`Done. (${installedMap.size} IDs, ${installedByName.size} names)`));

  process.stdout.write(chalk.gray('Loading all applications from winget... '));
  allApps = await winget.searchApp('');
  filteredApps = [...allApps];
  console.log(chalk.green(`Done. (${filteredApps.length} apps)\n`));

  const clamp = (value: number, min: number, max: number): number => Math.max(min, Math.min(max, value));
  const getAvailablePage = () => Math.floor(cursorIndex / PAGE_SIZE);
  const getSelectedPage = () => Math.floor(selectedCursorIndex / PAGE_SIZE);
  const getCurrentPageApps = () => filteredApps.slice(getAvailablePage() * PAGE_SIZE, getAvailablePage() * PAGE_SIZE + PAGE_SIZE);
  const getCurrentSelectedApps = () => selectedApps.slice(getSelectedPage() * PAGE_SIZE, getSelectedPage() * PAGE_SIZE + PAGE_SIZE);

  const mergeApps = (...appLists: InstalledApp[][]): InstalledApp[] => {
    const appsById = new Map<string, InstalledApp>();
    appLists.flat().forEach(app => {
      if (!app.id) return;
      const key = app.id.toLowerCase();
      if (!appsById.has(key)) appsById.set(key, app);
    });
    return Array.from(appsById.values());
  };

  const findInstalledMatch = (app: InstalledApp): InstalledMatch | undefined => {
    const installedById = installedMap.get(app.id.toLowerCase());
    if (installedById !== undefined) return installedById;
    const normalizedName = normalizeAppName(app.name);
    if (!normalizedName) return undefined;
    const exactNameMatch = installedByName.get(normalizedName);
    if (exactNameMatch !== undefined) return exactNameMatch;
    return installedNameEntries.find(installed =>
      installed.normalizedName.length >= 3 &&
      normalizedName.length >= 3 &&
      (installed.normalizedName.includes(normalizedName) || normalizedName.includes(installed.normalizedName))
    )?.match;
  };

  const clearScreen = () => process.stdout.write('\x1B[2J\x1B[H');
  const stripAnsi = (value: string): string => value.replace(/\x1B\[[0-?]*[ -/]*[@-~]/g, '');
  const safeText = (value: string, maxLength: number): string =>
    value
      .replace(/[^\x20-\x7E]/g, '?')
      .substring(0, maxLength);

  const printSideBySide = (tables: string[], gap = 3) => {
    const tableLines = tables.map(table => table.split('\n'));
    const widths = tableLines.map(lines => Math.max(...lines.map(line => stripAnsi(line).length)));
    const rowCount = Math.max(...tableLines.map(lines => lines.length));
    for (let rowIndex = 0; rowIndex < rowCount; rowIndex++) {
      const row = tableLines.map((lines, tableIndex) => {
        const line = lines[rowIndex] ?? '';
        const padding = tableIndex === tableLines.length - 1 ? '' : ' '.repeat(Math.max(0, widths[tableIndex] - stripAnsi(line).length + gap));
        return `${line}${padding}`;
      }).join('');
      console.log(row);
    }
  };

  const panelTitle = (panel: ActivePanel, title: string): string => activePanel === panel ? `> ${title}` : `  ${title}`;

  const buildAvailableTable = (): Table.Table => {
    const table = new Table({
      head: [' ', panelTitle('available', 'Available'), 'ID', 'Version', 'Source'],
      colWidths: [5, 18, 20, 9, 10],
      style: { head: activePanel === 'available' ? ['cyan'] : ['gray'], border: ['gray'] }
    });
    if (filteredApps.length === 0) {
      table.push([{ colSpan: 5, content: chalk.yellow('No applications found.') }]);
      return table;
    }
    const pageApps = getCurrentPageApps();
    const pageStart = getAvailablePage() * PAGE_SIZE;
    pageApps.forEach((app, i) => {
      const globalIdx = pageStart + i;
      const isSelected = selectedApps.some(s => s.id === app.id);
      const installedMatch = findInstalledMatch(app);
      const installedVersion = installedMatch?.version;
      const source = installedMatch?.source ?? app.source;
      const isInstalled = installedVersion !== undefined;
      const isCursor = activePanel === 'available' && globalIdx === cursorIndex;
      const statusIcon = isSelected ? (isInstalled ? chalk.green('[x]') : chalk.blue('[x]')) : (isInstalled ? chalk.green('[i]') : chalk.gray('[ ]'));
      const name = safeText(app.name, 16);
      const id = safeText(app.id, 18);
      const sourceText = safeText(source || 'N/A', 8);
      let versionText = safeText(app.version, 7);
      if (isInstalled && installedVersion !== app.version) versionText = chalk.yellow(versionText);
      else if (!isCursor) versionText = chalk.gray(versionText);
      if (isCursor) {
        const row = [chalk.bgCyan.black(statusIcon), chalk.bgCyan.black(name.padEnd(16)), chalk.bgCyan.black(id.padEnd(18)), chalk.bgCyan.black(safeText(app.version, 7).padEnd(7)), chalk.bgCyan.black(sourceText.padEnd(8))];
        if (isInstalled && installedVersion !== app.version) row[3] = chalk.bgCyan.yellow(safeText(app.version, 7).padEnd(7));
        table.push(row);
      } else {
        table.push([statusIcon, name, chalk.gray(id), versionText, chalk.gray(sourceText)]);
      }
    });
    return table;
  };

  const buildSelectedTable = (): Table.Table => {
    const table = new Table({ head: [panelTitle('selected', `Selected (${selectedApps.length})`), 'ID'], colWidths: [18, 20], style: { head: activePanel === 'selected' ? ['cyan'] : ['green'], border: ['gray'] } });
    if (selectedApps.length === 0) {
      table.push([{ colSpan: 2, content: chalk.gray('None selected') }]);
      return table;
    }
    const pageApps = getCurrentSelectedApps();
    const pageStart = getSelectedPage() * PAGE_SIZE;
    pageApps.forEach((app, index) => {
      const globalIndex = pageStart + index;
      const name = safeText(app.name, 16);
      const id = safeText(app.id, 18);
      if (activePanel === 'selected' && globalIndex === selectedCursorIndex) table.push([chalk.bgCyan.black(name.padEnd(16)), chalk.bgCyan.black(id.padEnd(18))]);
      else table.push([name, chalk.gray(id)]);
    });
    return table;
  };

  const render = () => {
    clearScreen();
    console.log(chalk.cyan.bold(`Creating config: ${configPath}`));
    console.log();
    const filterLabel = filterText ? `Filter: ${filterText}_ (${filteredApps.length} results)` : 'Filter: _';
    const installedFilterLabel = showInstalledOnly ? chalk.magenta(' [Installed only]') : '';
    console.log(chalk.yellow.bold(filterLabel) + installedFilterLabel);
    if (pendingAction) {
      console.log(chalk.yellow(`Confirm: ${pendingAction.message}`));
      console.log(chalk.gray('Enter = confirm, Esc = cancel'));
    }
    console.log();
    const totalPages = Math.max(1, Math.ceil(filteredApps.length / PAGE_SIZE));
    console.log(chalk.bold(`Applications (Page ${getAvailablePage() + 1}/${totalPages}):`));
    if (filterSearching) console.log(chalk.gray('  Searching winget source...'));
    printSideBySide([buildAvailableTable().toString(), buildSelectedTable().toString()]);
    console.log();
    console.log(chalk.gray('-'.repeat(70)));
    console.log(`${chalk.green('[i]')} installed  ${chalk.blue('[x]')} selected  ${chalk.gray('[ ]')} not selected`);
    console.log();
    console.log(chalk.bold('Controls:'));
    console.log(`  ${chalk.yellow('Type')}    Filter         ${chalk.yellow('Backspace')} Delete char   ${chalk.yellow('Tab')} Toggle installed filter`);
    console.log(`  ${chalk.yellow('Left/Right')} Change table   ${chalk.yellow('Up/Down')} Navigate`);
    console.log(`  ${chalk.yellow('Enter')}   Select/Remove  ${chalk.yellow('Ctrl+S')} Save   ${chalk.yellow('Ctrl+Q')} Quit`);
    console.log(`  ${chalk.yellow('Ctrl+I')} Install selected   ${chalk.yellow('Ctrl+U')} Uninstall selected`);
  };

  const getLocalMatches = (q: string): InstalledApp[] => allApps.filter(app => app.name.toLowerCase().includes(q) || app.id.toLowerCase().includes(q));
  const applyLocalFilter = () => {
    const trimmedFilter = filterText.trim();
    let apps = trimmedFilter ? getLocalMatches(trimmedFilter.toLowerCase()) : [...allApps];
    if (showInstalledOnly) {
      apps = apps.filter(app => findInstalledMatch(app) !== undefined);
    }
    filteredApps = apps;
    cursorIndex = clamp(cursorIndex, 0, Math.max(0, filteredApps.length - 1));
  };
  const applyWingetFilter = async (trimmedFilter: string) => {
    const q = trimmedFilter.toLowerCase();
    let wingetMatches = searchCache.get(q);
    if (wingetMatches === undefined) {
      filterSearching = true;
      render();
      wingetMatches = await winget.searchApp(trimmedFilter);
      searchCache.set(q, wingetMatches);
      filterSearching = false;
    }
    allApps = mergeApps(allApps, wingetMatches);
    if (filterText.trim().toLowerCase() === q) {
      filteredApps = mergeApps(getLocalMatches(q), wingetMatches);
      cursorIndex = clamp(cursorIndex, 0, Math.max(0, filteredApps.length - 1));
    }
  };
  const scheduleWingetFilter = () => {
    if (filterSearchTimer) clearTimeout(filterSearchTimer);
    const trimmedFilter = filterText.trim();
    if (trimmedFilter.length < 3) {
      filterSearching = false;
      return;
    }
    const token = ++filterSearchToken;
    filterSearchTimer = setTimeout(async () => {
      if (token !== filterSearchToken) return;
      await applyWingetFilter(trimmedFilter);
      if (token === filterSearchToken) render();
    }, 450);
  };

  const getActiveListLength = (): number => activePanel === 'available' ? filteredApps.length : selectedApps.length;
  const moveActiveCursor = (delta: number) => {
    const maxIndex = Math.max(0, getActiveListLength() - 1);
    if (activePanel === 'available') cursorIndex = clamp(cursorIndex + delta, 0, maxIndex);
    else selectedCursorIndex = clamp(selectedCursorIndex + delta, 0, maxIndex);
  };
  const changePanel = (delta: number) => {
    const panels: ActivePanel[] = ['available', 'selected'];
    activePanel = panels[clamp(panels.indexOf(activePanel) + delta, 0, panels.length - 1)];
  };
  const toggleAvailableApp = (app: InstalledApp) => {
    const selectedIndex = selectedApps.findIndex(s => s.id === app.id);
    if (selectedIndex === -1) {
      selectedApps.push({ id: app.id, name: app.name, version: app.version, availableInWinget: true });
      selectedCursorIndex = selectedApps.length - 1;
    } else {
      selectedApps.splice(selectedIndex, 1);
      selectedCursorIndex = clamp(selectedCursorIndex, 0, Math.max(0, selectedApps.length - 1));
    }
  };
  const executeEnterAction = () => {
    if (activePanel === 'available') {
      if (filteredApps.length === 0) return;
      const app = filteredApps[cursorIndex];
      toggleAvailableApp(app);
    } else {
      if (selectedApps.length === 0) return;
      selectedApps.splice(selectedCursorIndex, 1);
      selectedCursorIndex = clamp(selectedCursorIndex, 0, Math.max(0, selectedApps.length - 1));
    }
  };

  const save = () => {
    if (selectedApps.length === 0) return false;
    const config: Config = { apps: selectedApps };
    fs.writeFileSync(fullPath, JSON.stringify(config, null, 2));
    return true;
  };

  readline.emitKeypressEvents(process.stdin);
  if (process.stdin.isTTY) process.stdin.setRawMode(true);
  render();

  return new Promise((resolve) => {
    const cleanup = () => {
      if (filterSearchTimer) clearTimeout(filterSearchTimer);
      if (process.stdin.isTTY) process.stdin.setRawMode(false);
      process.stdin.removeAllListeners('keypress');
      clearScreen();
    };
    const saveAndExit = () => {
      if (save()) {
        cleanup();
        console.log(chalk.green(`Saved ${selectedApps.length} apps to ${fullPath}`));
        resolve();
      } else {
        clearScreen();
        console.log(chalk.red.bold('\n  No applications selected!\n'));
        setTimeout(() => render(), 1000);
      }
    };
    process.stdin.on('keypress', async (_str, key) => {
      if (!key) return;
      if (key.ctrl && key.name === 'c') {
        cleanup();
        console.log(chalk.yellow('Cancelled.'));
        process.exit(0);
      }
      if (pendingAction) {
        if (key.name === 'return') {
          const action = pendingAction;
          pendingAction = undefined;
          await action.run();
          render();
        } else if (key.name === 'escape' || key.name === 'n') {
          pendingAction = undefined;
          render();
        }
        return;
      }
      if (key.ctrl && key.name === 's') {
        saveAndExit();
        return;
      }
      if (key.ctrl && key.name === 'q') {
        cleanup();
        console.log(chalk.yellow('Exited without saving.'));
        resolve();
        return;
      }
      if (key.ctrl && key.name === 'i') {
        if (selectedApps.length === 0) { render(); return; }
        const appsToInstall = selectedApps.map(a => a.name).join(', ');
        pendingAction = {
          message: `Install/reinstall ${selectedApps.length} app(s)? (${appsToInstall})`,
          run: async () => {
            for (const app of selectedApps) {
              clearScreen();
              console.log(chalk.cyan(`Installing ${app.name}...`));
              const result = await winget.installApp(app.id);
              if (!result.success) {
                console.log(chalk.red(`Failed to install ${app.name}: ${result.message}`));
                await new Promise(resolve => setTimeout(resolve, 1500));
              }
            }
            clearScreen();
            console.log(chalk.gray('Refreshing installed applications list...'));
            installedApps = winget.getInstalledApps().filter(app => !winget.isSystemApp(app));
            rebuildInstalledIndexes();
            applyLocalFilter();
            render();
          }
        };
        render();
        return;
      }
      if (key.ctrl && key.name === 'u') {
        if (selectedApps.length === 0) { render(); return; }
        const installedSelected = selectedApps.filter(app => findInstalledMatch({ id: app.id, name: app.name, version: app.version, source: '' }) !== undefined);
        if (installedSelected.length === 0) {
          clearScreen();
          console.log(chalk.yellow('No selected apps are installed.'));
          await new Promise(resolve => setTimeout(resolve, 1500));
          render();
          return;
        }
        const appsToUninstall = installedSelected.map(a => a.name).join(', ');
        pendingAction = {
          message: `Uninstall ${installedSelected.length} app(s)? (${appsToUninstall})`,
          run: async () => {
            for (const app of installedSelected) {
              clearScreen();
              console.log(chalk.cyan(`Uninstalling ${app.name}...`));
              const result = await winget.uninstallApp(app.id);
              if (!result.success) {
                console.log(chalk.red(`Failed to uninstall ${app.name}: ${result.message}`));
                await new Promise(resolve => setTimeout(resolve, 1500));
              }
            }
            clearScreen();
            console.log(chalk.gray('Refreshing installed applications list...'));
            installedApps = winget.getInstalledApps().filter(app => !winget.isSystemApp(app));
            rebuildInstalledIndexes();
            applyLocalFilter();
            render();
          }
        };
        render();
        return;
      }
      if (key.name === 'tab') {
        showInstalledOnly = !showInstalledOnly;
        applyLocalFilter();
        render();
        return;
      }
      if (key.name === 'left') { changePanel(-1); render(); return; }
      if (key.name === 'right') { changePanel(1); render(); return; }
      if (key.name === 'up') { moveActiveCursor(-1); render(); return; }
      if (key.name === 'down') { moveActiveCursor(1); render(); return; }
      if (key.name === 'pageup') { moveActiveCursor(-PAGE_SIZE); render(); return; }
      if (key.name === 'pagedown') { moveActiveCursor(PAGE_SIZE); render(); return; }
      if (key.name === 'return') { executeEnterAction(); render(); return; }
      if (key.name === 'escape') { render(); return; }
      if (key.name === 'backspace') {
        filterText = filterText.slice(0, -1);
        applyLocalFilter();
        scheduleWingetFilter();
        render();
        return;
      }
      if (!key.ctrl && !key.meta && key.sequence && key.sequence.length === 1 && key.sequence.charCodeAt(0) >= 32) {
        filterText += key.sequence;
        applyLocalFilter();
        scheduleWingetFilter();
        render();
      }
    });
  });
}
