import { execSync, spawn } from 'child_process';
import { InstalledApp } from '../types/config';

// Patterns to exclude system apps, drivers, and runtimes
const EXCLUDED_PATTERNS = [
  /^Microsoft\.NET/i,
  /^Microsoft\.VCRedist/i,
  /^Microsoft\.VC\+\+/i,
  /^Microsoft\.UI\.Xaml/i,
  /^Microsoft\.Windows/i,
  /^Microsoft\.DirectX/i,
  /^Microsoft\.GameInput/i,
  /^Microsoft\.Update/i,
  /^Microsoft\.VSS/i,
  /^Microsoft\.ODBC/i,
  /^Microsoft\.OLE/i,
  /^Microsoft\.Help/i,
  /^Microsoft\.SQL.*Setup/i,
  /^Microsoft\.VisualStudio\.Installer/i,
  /^Microsoft\.VisualStudio\.Tools/i,
  /WindowsAppRuntime/i,
  /WindowsDesktopRuntime/i,
  /^ARP\\/i,
  /^MSIX\\/i,
  /Driver/i,
  /^NVIDIA\.Control/i,
  /^Realtek/i,
  /^Synaptics/i,
  /^Intel\./i,
  /^AMD\./i,
  /Redistributable/i,
  /Runtime Package/i,
  /\.Net.*Runtime/i,
  /^dotnet/i,
  /^Microsoft\.Advertising/i,
  /^Microsoft\.Services/i,
  /^Microsoft\.StorePurchase/i,
  /^Microsoft\.VP9/i,
  /^Microsoft\.HEVC/i,
  /^Microsoft\.AV1/i,
  /^Microsoft\.MPEG/i,
  /^Microsoft\.WebMedia/i,
  /^Microsoft\.WebP/i,
  /^Microsoft\.Raw/i,
  /^Microsoft\.HEIFImage/i,
  /Local Experience Pack/i,
  /Language Pack/i,
  /Speech Pack/i,
  /Pakiet lokalizacyjny/i,
  /本地体验包/i,
  /^Microsoft\.Xbox.*Provider/i,
  /^Microsoft\.Xbox.*Plugin/i,
  /^Microsoft\.Gaming/i,
  /^Microsoft\.Wallet/i,
  /^Microsoft\.People/i,
  /^Microsoft\.GetHelp/i,
  /^Microsoft\.Getstarted/i,
  /^Microsoft\.MixedReality/i,
  /^Microsoft\.549981/i, // Cortana
  /^Microsoft\.BingNews/i,
  /^Microsoft\.BingWeather/i,
  /^Microsoft\.ZuneMusic/i,
  /^Microsoft\.ZuneVideo/i,
  /Widget.*Runtime/i,
  /Host środowiska/i,
  /Usługi gier/i,
];

export class WingetService {
  isSystemApp(app: InstalledApp): boolean {
    for (const pattern of EXCLUDED_PATTERNS) {
      if (pattern.test(app.id) || pattern.test(app.name)) {
        return true;
      }
    }
    return false;
  }

  private runCommand(command: string): string {
    try {
      return execSync(command, {
        encoding: 'utf-8',
        maxBuffer: 50 * 1024 * 1024,
        windowsHide: true
      });
    } catch (error: any) {
      if (error.stdout) {
        return error.stdout;
      }
      throw error;
    }
  }

  getInstalledApps(): InstalledApp[] {
    const rawOutput = this.runCommand('winget list --disable-interactivity');
    return this.parseWingetTable(rawOutput);
  }

  async isAvailableInWinget(id: string): Promise<boolean> {
    try {
      const output = this.runCommand(`winget search --id "${id}" --exact --disable-interactivity`);
      return output.includes(id);
    } catch {
      return false;
    }
  }

  async installApp(id: string): Promise<{ success: boolean; message: string }> {
    return new Promise((resolve) => {
      const process = spawn('winget', ['install', '--id', id, '--silent', '--accept-source-agreements', '--accept-package-agreements', '--disable-interactivity'], {
        shell: true,
        stdio: ['ignore', 'pipe', 'pipe']
      });

      let output = '';
      let errorOutput = '';

      process.stdout?.on('data', (data) => {
        output += data.toString();
        const text = data.toString().trim();
        if (text) console.log(`  ${text}`);
      });

      process.stderr?.on('data', (data) => {
        errorOutput += data.toString();
      });

      process.on('close', (code) => {
        if (code === 0) {
          resolve({ success: true, message: 'Installed successfully' });
        } else if (output.includes('already installed')) {
          resolve({ success: true, message: 'Already installed' });
        } else {
          resolve({ success: false, message: errorOutput || output || 'Installation failed' });
        }
      });

      process.on('error', (error) => {
        resolve({ success: false, message: error.message });
      });
    });
  }

  async uninstallApp(id: string): Promise<{ success: boolean; message: string }> {
    return new Promise((resolve) => {
      const process = spawn('winget', ['uninstall', '--id', id, '--silent', '--disable-interactivity'], {
        shell: true,
        stdio: ['ignore', 'pipe', 'pipe']
      });

      let output = '';
      let errorOutput = '';

      process.stdout?.on('data', (data) => {
        output += data.toString();
        const text = data.toString().trim();
        if (text) console.log(`  ${text}`);
      });

      process.stderr?.on('data', (data) => {
        errorOutput += data.toString();
      });

      process.on('close', (code) => {
        if (code === 0) {
          resolve({ success: true, message: 'Uninstalled successfully' });
        } else {
          resolve({ success: false, message: errorOutput || output || 'Uninstall failed' });
        }
      });

      process.on('error', (error) => {
        resolve({ success: false, message: error.message });
      });
    });
  }

  async searchApp(query: string): Promise<InstalledApp[]> {
    try {
      const cmd = query
        ? `winget search "${query}" --disable-interactivity`
        : `winget search "" --disable-interactivity`;

      const output = this.runCommand(cmd);
      return this.parseWingetTable(output);
    } catch {
      return [];
    }
  }

  private parseWingetTable(rawOutput: string): InstalledApp[] {
    // Split by both \r\n and \n, then filter out progress spinner lines
    // Winget uses \r to overwrite progress lines, so we need to handle this
    const lines = rawOutput
      .split(/\r?\n/)
      .map(line => {
        // If line contains \r (progress updates), take only the last segment
        if (line.includes('\r')) {
          const segments = line.split('\r');
          return segments[segments.length - 1];
        }
        return line;
      })
      .filter(line => line.trim().length > 0);

    const separatorIndex = lines.findIndex(line =>
      /^\s*-{3,}[\s-]*$/.test(line)
    );

    if (separatorIndex === -1) return [];

    const headerLine = lines[separatorIndex - 1] ?? '';
    const separatorLine = lines[separatorIndex];
    const headerMatches = Array.from(headerLine.matchAll(/\S+/g));
    const separatorMatches = Array.from(separatorLine.matchAll(/-+/g));

    const columnStarts = headerMatches.length >= 2
      ? headerMatches.map(match => match.index ?? 0)
      : separatorMatches.map(match => match.index ?? 0);

    if (columnStarts.length < 2) return [];

    const apps: InstalledApp[] = [];
    for (let i = separatorIndex + 1; i < lines.length; i++) {
      const line = lines[i];
      if (!line.trim() || line.trim().startsWith('---')) continue;

      const columns = columnStarts.map((start, index) => {
        const end = columnStarts[index + 1] ?? line.length;
        return line.substring(start, end).trim();
      });

      const name = columns[0] ?? '';
      const id = columns[1] ?? '';
      const version = columns[2] ?? '';
      const source = columns[columnStarts.length - 1] ?? '';

      if (name && id) {
        apps.push({ name, id, version, source });
      }
    }
    return apps;
  }
}
