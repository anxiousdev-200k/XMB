import { call } from "./api.js";

const ACTIONS = [
  { id: "resume", label: "Resume Game" },
  { id: "volume", label: "System Volume", value: "Adjust with ◀ ▶" },
  { id: "output", label: "Select Output Device" },
  { id: "sleep", label: "Enter Rest Mode" },
  { id: "shutdown", label: "Turn Off System" },
];

export class GameQuickMenu {
  constructor(audio, { onResume, onShutdown }) {
    this.audio = audio;
    this.onResume = onResume;
    this.onShutdown = onShutdown;
    this.el = document.getElementById("quickmenu");
    this.selected = 0;
  }
  open() {
    this.selected = 0;
    this.el.setAttribute("aria-hidden", "false");
    this.el.classList.add("show");
    this.render();
  }
  close() {
    this.el.classList.remove("show");
    this.el.setAttribute("aria-hidden", "true");
  }
  render() {
    this.el.innerHTML = `<div class="confirm quickmenu-content">
      <div class="confirm-title">Quick Menu</div>
      <div class="confirm-opts"></div>
    </div>
    <div class="confirm-foot quickmenu-foot">
      <span class="cf-hint"><span class="cf-glyph cf-x">✕</span>Select</span>
      <span class="cf-hint"><span class="cf-glyph cf-o">○</span>Resume Game</span>
      <span class="cf-hint">↑ ↓ Volume</span>
    </div>`;
    const list = this.el.querySelector(".confirm-opts");
    for (const [i, item] of ACTIONS.entries()) {
      const option = document.createElement("button");
      option.type = "button";
      option.className = `confirm-opt quickmenu-opt${i === this.selected ? " sel" : ""}`;
      option.textContent = item.label;
      option.addEventListener("click", () => {
        this.selected = i;
        this.render();
        this.audio.play("enter", 0.6);
        this._activate();
      });
      list.appendChild(option);
    }
  }
  async _activate() {
    switch (ACTIONS[this.selected].id) {
      case "resume": this.close(); this.onResume(); break;
      case "volume": await this.adjustVolume(1); break;
      case "output":
        try { await call("open_sound_settings"); }
        catch (e) { console.error("Opening sound settings failed", e); this.audio.play("error", 0.5); }
        break;
      case "sleep":
        try { await call("sleep_system"); }
        catch (e) { console.error("System sleep failed", e); this.audio.play("error", 0.5); }
        break;
      case "shutdown": this.close(); this.onShutdown(); break;
    }
  }
  async adjustVolume(steps) {
    try { await call("adjust_system_volume", { steps }); }
    catch (e) { console.error("System volume adjustment failed", e); this.audio.play("error", 0.5); }
  }
  onInput(action) {
    if (!this.el.classList.contains("show")) return;
    if (this.selected === 1 && (action === "up" || action === "down")) {
      this.adjustVolume(action === "up" ? 1 : -1);
    } else if (action === "up" || action === "down") {
      this.selected = (this.selected + (action === "down" ? 1 : ACTIONS.length - 1)) % ACTIONS.length;
      this.audio.play(action === "down" ? "cursor_down" : "cursor_up", 0.5);
      this.render();
    } else if (action === "left" || action === "right") {
      this.selected = (this.selected + (action === "right" ? 1 : ACTIONS.length - 1)) % ACTIONS.length;
      this.audio.play(action === "right" ? "cursor_down" : "cursor_up", 0.5);
      this.render();
    } else if (action === "enter") {
      this.audio.play("enter", 0.6);
      this._activate();
    } else if (action === "back") {
      this.audio.play("back", 0.5);
      this.close();
      this.onResume();
    }
  }
}
