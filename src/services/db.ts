import Database from "@tauri-apps/plugin-sql";
import { invoke } from "@tauri-apps/api/core";

let dbInstance: Database | null = null;
let initPromise: Promise<Database> | null = null;

export async function getDb(): Promise<Database> {
  if (dbInstance) return dbInstance;
  if (!initPromise) {
    initPromise = (async () => {
      const db = await Database.load("sqlite:nishikanto.db");
      await initTables(db);
      dbInstance = db;
      return db;
    })();
  }
  return initPromise;
}

async function initTables(db: Database) {
  // 1. Personal Dictionary
  await db.execute(`
    CREATE TABLE IF NOT EXISTS personal_dictionary (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      word TEXT NOT NULL UNIQUE,
      created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );
  `);

  // 2. App Exclusions (Blacklist)
  await db.execute(`
    CREATE TABLE IF NOT EXISTS app_exclusions (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      app_name TEXT NOT NULL UNIQUE,
      display_name TEXT,
      created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );
  `);

  // 3. User Settings (Goals, API Key, Preferences)
  await db.execute(`
    CREATE TABLE IF NOT EXISTS user_settings (
      key TEXT PRIMARY KEY,
      value TEXT NOT NULL
    );
  `);

  // Seed default exclusions if table is empty
  try {
    const exclusions = await db.select<{ app_name: string }[]>(
      "SELECT app_name FROM app_exclusions LIMIT 1"
    );
    if (exclusions.length === 0) {
      await db.execute(
        "INSERT OR IGNORE INTO app_exclusions (app_name, display_name) VALUES ('code.exe', 'Visual Studio Code'), ('windowsterminal.exe', 'Windows Terminal'), ('iterm2', 'iTerm2');"
      );
    }
  } catch (e) {
    console.warn("Could not seed default exclusions:", e);
  }

  // Initial sync with Rust engine
  await syncWithRust(db);
}

export async function syncWithRust(db?: Database) {
  try {
    const activeDb = db || (await getDb());

    // Sync dictionary words to Rust
    const dictRows = await activeDb.select<{ word: string }[]>(
      "SELECT word FROM personal_dictionary ORDER BY word ASC"
    );
    const words = dictRows.map((r) => r.word);
    await invoke("sync_dictionary", { words });

    // Sync app exclusions to Rust
    const appRows = await activeDb.select<{ app_name: string }[]>(
      "SELECT app_name FROM app_exclusions ORDER BY app_name ASC"
    );
    const apps = appRows.map((r) => r.app_name);
    await invoke("sync_app_exclusions", { apps });

    // Sync custom api key if stored in SQLite
    const keyRows = await activeDb.select<{ value: string }[]>(
      "SELECT value FROM user_settings WHERE key = 'gemini_api_key' LIMIT 1"
    );
    if (keyRows.length > 0 && keyRows[0].value) {
      await invoke("set_custom_api_key", { key: keyRows[0].value });
    }
  } catch (err) {
    console.warn("Could not sync with Rust engine:", err);
  }
}

// -------------------------------------------------------------
// Personal Dictionary Operations
// -------------------------------------------------------------
export async function getPersonalDictionary(): Promise<string[]> {
  try {
    const db = await getDb();
    const rows = await db.select<{ word: string }[]>(
      "SELECT word FROM personal_dictionary ORDER BY word ASC"
    );
    return rows.map((r) => r.word);
  } catch {
    return [];
  }
}

export async function addWordToDictionary(word: string): Promise<string[]> {
  const clean = word.trim().toLowerCase();
  if (!clean) return getPersonalDictionary();

  try {
    const db = await getDb();
    await db.execute(
      "INSERT OR IGNORE INTO personal_dictionary (word) VALUES (?)",
      [clean]
    );
    await invoke("add_to_dictionary", { word: clean });
  } catch (e) {
    console.warn("Could not insert word into personal dictionary:", e);
  }
  return getPersonalDictionary();
}

export async function removeWordFromDictionary(word: string): Promise<string[]> {
  const clean = word.trim().toLowerCase();
  try {
    const db = await getDb();
    await db.execute("DELETE FROM personal_dictionary WHERE word = ?", [clean]);
    await invoke("remove_from_dictionary", { word: clean });
  } catch (e) {
    console.warn("Could not remove word from personal dictionary:", e);
  }
  return getPersonalDictionary();
}

// -------------------------------------------------------------
// App Exclusions (Blacklist) Operations
// -------------------------------------------------------------
export interface AppExclusionItem {
  id?: number;
  app_name: string;
  display_name: string;
}

export async function getAppExclusions(): Promise<AppExclusionItem[]> {
  try {
    const db = await getDb();
    return await db.select<AppExclusionItem[]>(
      "SELECT id, app_name, COALESCE(display_name, app_name) as display_name FROM app_exclusions ORDER BY app_name ASC"
    );
  } catch {
    return [];
  }
}

export async function addAppExclusion(
  appName: string,
  displayName?: string
): Promise<AppExclusionItem[]> {
  const clean = appName.trim().toLowerCase();
  if (!clean) return getAppExclusions();

  const disp = displayName?.trim() || appName.trim();
  try {
    const db = await getDb();
    await db.execute(
      "INSERT OR IGNORE INTO app_exclusions (app_name, display_name) VALUES (?, ?)",
      [clean, disp]
    );
    await invoke("add_app_exclusion", { appName: clean });
  } catch (e) {
    console.warn("Could not add app exclusion:", e);
  }
  return getAppExclusions();
}

export async function removeAppExclusion(appName: string): Promise<AppExclusionItem[]> {
  const clean = appName.trim().toLowerCase();
  try {
    const db = await getDb();
    await db.execute("DELETE FROM app_exclusions WHERE app_name = ?", [clean]);
    await invoke("remove_app_exclusion", { appName: clean });
  } catch (e) {
    console.warn("Could not remove app exclusion:", e);
  }
  return getAppExclusions();
}

// -------------------------------------------------------------
// User Settings (Writing Goals & Gemini API Key) Operations
// -------------------------------------------------------------
export async function getSetting(key: string, defaultValue = ""): Promise<string> {
  try {
    const db = await getDb();
    const rows = await db.select<{ value: string }[]>(
      "SELECT value FROM user_settings WHERE key = ? LIMIT 1",
      [key]
    );
    return rows.length > 0 ? rows[0].value : defaultValue;
  } catch {
    return defaultValue;
  }
}

export async function saveSetting(key: string, value: string): Promise<void> {
  try {
    const db = await getDb();
    await db.execute(
      "INSERT INTO user_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
      [key, value]
    );
    if (key === "gemini_api_key") {
      await invoke("set_custom_api_key", { key });
    }
  } catch (e) {
    console.warn(`Could not save setting ${key}:`, e);
  }
}
