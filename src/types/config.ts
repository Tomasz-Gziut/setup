export interface AppConfig {
  id: string;
  name: string;
  version: string;
  availableInWinget: boolean;
  note?: string;
}

export interface Config {
  apps: AppConfig[];
}

export interface InstalledApp {
  name: string;
  id: string;
  version: string;
  source: string;
}
