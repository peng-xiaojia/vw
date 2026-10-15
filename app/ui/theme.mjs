const KEY = 'vw.appearance';
const defaults = { mode: 'system', darkHour: '19:00', lightHour: '07:00', fontFamily: 'system', fontSize: 15 };

export function effectiveTheme(settings, darkSystem, now = new Date()) {
  if (settings.mode === 'light' || settings.mode === 'dark') return settings.mode;
  if (settings.mode !== 'schedule') return darkSystem ? 'dark' : 'light';
  const clock = now.getHours() * 60 + now.getMinutes();
  const minutes = value => { const [hour, minute] = value.split(':').map(Number); return hour * 60 + minute; };
  const start = minutes(settings.darkHour), end = minutes(settings.lightHour);
  if (start === end) return darkSystem ? 'dark' : 'light';
  return (start < end ? clock >= start && clock < end : clock >= start || clock < end) ? 'dark' : 'light';
}

export class ThemeSettings {
  constructor() {
    try { this.settings = { ...defaults, ...JSON.parse(localStorage.getItem(KEY) || '{}') }; }
    catch { this.settings = { ...defaults }; }
    if (!['system', 'light', 'dark', 'schedule'].includes(this.settings.mode)) this.settings.mode = 'system';
    if (!['system', 'serif', 'mono'].includes(this.settings.fontFamily)) this.settings.fontFamily = 'system';
    this.settings.fontSize = Math.max(12, Math.min(28, Number(this.settings.fontSize) || 15));
    this.system = matchMedia('(prefers-color-scheme: dark)');
    this.apply();
    this.system.addEventListener('change', () => this.apply());
    this.timer = setInterval(() => this.apply(), 60_000);
  }
  apply() {
    document.documentElement.dataset.theme = effectiveTheme(this.settings, this.system.matches);
    document.documentElement.dataset.documentFont = this.settings.fontFamily;
    document.documentElement.style.setProperty('--document-font-size', `${this.settings.fontSize}px`);
  }
  save() { localStorage.setItem(KEY, JSON.stringify(this.settings)); this.apply(); }
  adjustFontSize(step) {
    const next = Math.max(12, Math.min(28, this.settings.fontSize + Math.sign(step)));
    if (next === this.settings.fontSize) return;
    this.settings.fontSize = next;
    document.getElementById('document-font-size').value = String(next);
    document.getElementById('document-font-size-value').value = `${next} px`;
    this.save();
  }
  wire() {
    const dialog = document.getElementById('appearance-dialog');
    dialog.querySelector(`[name="theme-mode"][value="${this.settings.mode}"]`).checked = true;
    document.getElementById('theme-dark-hour').value = this.settings.darkHour;
    document.getElementById('theme-light-hour').value = this.settings.lightHour;
    document.getElementById('document-font-family').value = this.settings.fontFamily;
    document.getElementById('document-font-size').value = String(this.settings.fontSize);
    document.getElementById('document-font-size-value').value = `${this.settings.fontSize} px`;
    dialog.addEventListener('change', event => {
      if (event.target.name === 'theme-mode') this.settings.mode = event.target.value;
      if (event.target.id === 'theme-dark-hour') this.settings.darkHour = event.target.value;
      if (event.target.id === 'theme-light-hour') this.settings.lightHour = event.target.value;
      if (event.target.id === 'document-font-family') this.settings.fontFamily = event.target.value;
      document.getElementById('theme-schedule').hidden = this.settings.mode !== 'schedule';
      this.save();
    });
    document.getElementById('document-font-size').addEventListener('input', event => {
      this.settings.fontSize = Number(event.target.value);
      document.getElementById('document-font-size-value').value = `${this.settings.fontSize} px`;
      this.save();
    });
    document.getElementById('theme-schedule').hidden = this.settings.mode !== 'schedule';
  }
}
