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

type ActivePanel = 'available' | 'selected' | 'installed';

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
  let installedCursorIndex = 0;
  let filterText = '';
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
  const getInstalledPage = () => Math.floor(installedCursorIndex / PAGE_SIZE);
  const getCurrentPageApps = () => filteredApps.slice(getAvailablePage() * PAGE_SIZE, getAvailablePage() * PAGE_SIZE + PAGE_SIZE);
  const getCurrentSelectedApps = () => selectedApps.slice(getSelectedPage() * PAGE_SIZE, getSelectedPage() * PAGE_SIZE + PAGE_SIZE);
  const getCurrentInstalledApps = () => installedApps.slice(getInstalledPage() * PAGE_SIZE, getInstalledPage() * PAGE_SIZE + PAGE_SIZE);

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
      colWidths: [5, 22, 24, 10, 11],
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
      const statusIcon = isInstalled ? chalk.green('[i]') : isSelected ? chalk.blue('[x]') : chalk.gray('[ ]');
      const name = safeText(app.name, 20);
      const id = safeText(app.id, 22);
      const sourceText = safeText(source || 'N/A', 9);
      let versionText = safeText(app.version, 8);
      if (isInstalled && installedVersion !== app.version) versionText = chalk.yellow(versionText);
      else if (!isCursor) versionText = chalk.gray(versionText);
      if (isCursor) {
        const row = [chalk.bgCyan.black(statusIcon), chalk.bgCyan.black(name.padEnd(20)), chalk.bgCyan.black(id.padEnd(22)), chalk.bgCyan.black(safeText(app.version, 8).padEnd(8)), chalk.bgCyan.black(sourceText.padEnd(9))];
        if (isInstalled && installedVersion !== app.version) row[3] = chalk.bgCyan.yellow(safeText(app.version, 8).padEnd(8));
        table.push(row);
      } else {
        table.push([statusIcon, name, chalk.gray(id), versionText, chalk.gray(sourceText)]);
      }
    });
    return table;
  };

  const buildSelectedTable = (): Table.Table => {
    const table = new Table({ head: ['#', panelTitle('selected', `Selected (${selectedApps.length})`)], colWidths: [5, 25], style: { head: activePanel === 'selected' ? ['cyan'] : ['green'], border: ['gray'] } });
    if (selectedApps.length === 0) {
      table.push([{ colSpan: 2, content: chalk.gray('None selected') }]);
      return table;
    }
    const pageApps = getCurrentSelectedApps();
    const pageStart = getSelectedPage() * PAGE_SIZE;
    pageApps.forEach((app, index) => {
      const globalIndex = pageStart + index;
      const label = String(globalIndex + 1);
      const name = safeText(app.name, 23);
      if (activePanel === 'selected' && globalIndex === selectedCursorIndex) table.push([chalk.bgCyan.black(label.padEnd(3)), chalk.bgCyan.black(name.padEnd(23))]);
      else table.push([label, name]);
    });
    return table;
  };

  const buildInstalledTable = (): Table.Table => {
    const table = new Table({ head: ['#', panelTitle('installed', `Installed (${installedApps.length})`), 'Source'], colWidths: [5, 24, 12], style: { head: activePanel === 'installed' ? ['cyan'] : ['magenta'], border: ['gray'] } });
    if (installedApps.length === 0) {
      table.push([{ colSpan: 3, content: chalk.gray('No installed apps') }]);
      return table;
    }
    const pageApps = getCurrentInstalledApps();
    const pageStart = getInstalledPage() * PAGE_SIZE;
    pageApps.forEach((app, index) => {
      const globalIndex = pageStart + index;
      const label = String(globalIndex + 1);
      const name = safeText(app.name, 22);
      const source = safeText(app.source || 'local/other', 10);
      if (activePanel === 'installed' && globalIndex === installedCursorIndex) table.push([chalk.bgCyan.black(label.padEnd(3)), chalk.bgCyan.black(name.padEnd(22)), chalk.bgCyan.black(source.padEnd(10))]);
      else table.push([label, name, chalk.gray(source)]);
    });
    return table;
  };

  const render = () => {
    clearScreen();
    console.log(chalk.cyan.bold(`Creating config: ${configPath}`));
    console.log();
    const filterLabel = filterText ? `Filter: ${filterText}_ (${filteredApps.length} results)` : 'Filter: _';
    console.log(chalk.yellow.bold(filterLabel));
    if (pendingAction) {
      console.log(chalk.yellow(`Confirm: ${pendingAction.message}`));
      console.log(chalk.gray('Enter = confirm, Esc = cancel'));
    }
    console.log();
    const totalPages = Math.max(1, Math.ceil(filteredApps.length / PAGE_SIZE));
    console.log(chalk.bold(`Applications (Page ${getAvailablePage() + 1}/${totalPages}):`));
    if (filterSearching) console.log(chalk.gray('  Searching winget source...'));
    const sidePanel = `${buildSelectedTable().toString()}\n\n${buildInstalledTable().toString()}`;
    printSideBySide([buildAvailableTable().toString(), sidePanel]);
    console.log();
    console.log(chalk.gray('-'.repeat(60)));
    console.log(`${chalk.green('[i]')} installed  ${chalk.blue('[x]')} selected  ${chalk.gray('[ ]')} not selected`);
    console.log();
    console.log(chalk.bold('Controls:'));
    console.log(`  ${chalk.yellow('Type')}    Filter         ${chalk.yellow('Backspace')} Delete char`);
    console.log(`  ${chalk.yellow('Left/Right')} Change table   ${chalk.yellow('Up/Down')} Navigate`);
    console.log(`  ${chalk.yellow('Enter')}   Action         ${chalk.yellow('Ctrl+S')} Save   ${chalk.yellow('Ctrl+Q')} Quit`);
  };

  const getLocalMatches = (q: string): InstalledApp[] => allApps.filter(app => app.name.toLowerCase().includes(q) || app.id.toLowerCase().includes(q));
  const applyLocalFilter = () => {
    const trimmedFilter = filterText.trim();
    filteredApps = trimmedFilter ? getLocalMatches(trimmedFilter.toLowerCase()) : [...allApps];
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

  const getActiveListLength = (): number => activePanel === 'available' ? filteredApps.length : activePanel === 'selected' ? selectedApps.length : installedApps.length;
  const moveActiveCursor = (delta: number) => {
    const maxIndex = Math.max(0, getActiveListLength() - 1);
    if (activePanel === 'available') cursorIndex = clamp(cursorIndex + delta, 0, maxIndex);
    else if (activePanel === 'selected') selectedCursorIndex = clamp(selectedCursorIndex + delta, 0, maxIndex);
    else installedCursorIndex = clamp(installedCursorIndex + delta, 0, maxIndex);
  };
  const changePanel = (delta: number) => {
    const panels: ActivePanel[] = ['available', 'selected', 'installed'];
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
  const prepareEnterAction = () => {
    if (activePanel === 'available') {
      if (filteredApps.length === 0) return;
      const app = filteredApps[cursorIndex];
      const isSelected = selectedApps.some(s => s.id === app.id);
      pendingAction = { message: `${isSelected ? 'Remove from selected' : 'Select'} ${app.name}?`, run: () => toggleAvailableApp(app) };
    } else if (activePanel === 'selected') {
      if (selectedApps.length === 0) return;
      const app = selectedApps[selectedCursorIndex];
      pendingAction = { message: `Remove ${app.name} from selected?`, run: () => { selectedApps.splice(selectedCursorIndex, 1); selectedCursorIndex = clamp(selectedCursorIndex, 0, Math.max(0, selectedApps.length - 1)); } };
    } else {
      if (installedApps.length === 0) return;
      const app = installedApps[installedCursorIndex];
      pendingAction = { message: `Uninstall ${app.name}?`, run: async () => {
        const result = await winget.uninstallApp(app.id);
        if (result.success) {
          installedApps = installedApps.filter(installed => installed.id !== app.id);
          installedCursorIndex = clamp(installedCursorIndex, 0, Math.max(0, installedApps.length - 1));
          rebuildInstalledIndexes();
        } else {
          clearScreen();
          console.log(chalk.red(`Uninstall failed: ${result.message}`));
          await new Promise(resolve => setTimeout(resolve, 1500));
        }
      } };
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
      if (key.name === 'left') { changePanel(-1); render(); return; }
      if (key.name === 'right') { changePanel(1); render(); return; }
      if (key.name === 'up') { moveActiveCursor(-1); render(); return; }
      if (key.name === 'down') { moveActiveCursor(1); render(); return; }
      if (key.name === 'pageup') { moveActiveCursor(-PAGE_SIZE); render(); return; }
      if (key.name === 'pagedown') { moveActiveCursor(PAGE_SIZE); render(); return; }
      if (key.name === 'return') { prepareEnterAction(); render(); return; }
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
