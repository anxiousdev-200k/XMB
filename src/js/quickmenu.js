import { call } from "./api.js";

const ACTIONS = [
  { id: "resume", label: "Resume Game" },
  { id: "volume", label: "System Volume" },
  { id: "output", label: "Switch Output Device" },
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
    this.outputStatus = "";
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
      <div class="confirm-opts"></div>
      <div class="quickmenu-status"></div>
    </div>
    <div class="confirm-foot quickmenu-foot">
      <span class="cf-hint"><span class="cf-glyph cf-x">✕</span>Select</span>
      <span class="cf-hint"><span class="cf-glyph cf-o">○</span>Resume Game</span>
      <span class="cf-hint">← → Volume</span>
    </div>`;
    this.el.querySelector(".quickmenu-status").textContent = this.outputStatus;
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
      case "resume": this.onResume(); break;
      case "volume": await this.adjustVolume(1); break;
      case "output":
        try {
          this.outputStatus = `Output: ${await call("switch_output_device")}`;
          this.render();
        } catch (e) {
          console.error("Switching output device failed", e);
          this.outputStatus = `Output switch failed: ${e instanceof Error ? e.message : String(e)}`;
          this.render();
          this.audio.play("error", 0.5);
        }
        break;
      case "sleep":
        try { await call("sleep_system"); }
        catch (e) {
          console.error("System sleep failed", e);
          this.outputStatus = `Rest Mode failed: ${e instanceof Error ? e.message : String(e)}`;
          this.render();
          this.audio.play("error", 0.5);
        }
        break;
      case "shutdown": this.close(); this.onShutdown(); break;
    }
  }
  async adjustVolume(steps) {
    try { await call("adjust_system_volume", { steps }); }
    catch (e) {
      console.error("System volume adjustment failed", e);
      this.outputStatus = `Volume adjustment failed: ${e instanceof Error ? e.message : String(e)}`;
      this.render();
      this.audio.play("error", 0.5);
    }
  }
  onInput(action) {
    if (!this.el.classList.contains("show")) return;
    if (action === "up" || action === "down") {
      this.selected = (this.selected + (action === "down" ? 1 : ACTIONS.length - 1)) % ACTIONS.length;
      this.audio.play(action === "down" ? "cursor_down" : "cursor_up", 0.5);
      this.render();
    } else if (action === "left" || action === "right") {
      if (this.selected === 1) this.adjustVolume(action === "right" ? 1 : -1);
    } else if (action === "enter") {
      this.audio.play("enter", 0.6);
      this._activate();
    } else if (action === "back") {
      this.audio.play("back", 0.5);
      this.onResume();
    }
  }
}
