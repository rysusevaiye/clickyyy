const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const volume = $("volume");
const muted = $("muted");
const closeToTray = $("close-to-tray");
const startMinimized = $("start-minimized");
const output = $("output");
let permission = "granted";
let outputDevice = null;
let deviceNames = [];

function renderOutput() {
  const options = [new Option("System default", ""), ...deviceNames.map((name) => new Option(name, name))];
  if (outputDevice && !deviceNames.includes(outputDevice)) {
    options.push(new Option(`${outputDevice} (not connected)`, outputDevice));
  }
  output.replaceChildren(...options);
  output.value = outputDevice ?? "";
}

async function refreshDevices() {
  const names = await invoke("list_output_devices");
  if (JSON.stringify(names) === JSON.stringify(deviceNames)) return;
  deviceNames = names;
  renderOutput();
}

function setVolumeLabel(percent) {
  $("volume-value").textContent = `${percent}%`;
  volume.style.setProperty("--fill", `${percent}%`);
}

function render(state) {
  permission = state.permission;
  if (state.outputDevice !== outputDevice) {
    outputDevice = state.outputDevice;
    renderOutput();
  }
  const percent = Math.round(state.volume * 100);
  if (document.activeElement !== volume) volume.value = percent;
  setVolumeLabel(percent);
  muted.checked = state.muted;
  volume.disabled = state.muted;
  closeToTray.checked = state.closeToTray;
  startMinimized.checked = state.startMinimized;
  startMinimized.disabled = !state.closeToTray;
  $("hide").disabled = !state.closeToTray;
  $("version").textContent = `v${state.version}`;
  document.querySelectorAll("[data-profile]").forEach((button) => {
    button.setAttribute("aria-checked", String(button.dataset.profile === state.profile));
  });

  const status = $("status");
  if (state.permission !== "granted") {
    status.textContent = "Inactive";
    status.className = "badge inactive";
  } else if (state.muted) {
    status.textContent = "Muted";
    status.className = "badge muted";
  } else {
    status.textContent = "Active";
    status.className = "badge active";
  }

  const banner = $("permission");
  banner.hidden = state.permission === "granted";
  if (state.permission === "relaunch") {
    $("permission-text").textContent = "Input Monitoring is enabled. Relaunch Clicky to start listening.";
    $("permission-action").textContent = "Relaunch Clicky";
  } else if (state.permission === "waiting") {
    $("permission-text").textContent =
      "Clicky needs Input Monitoring access to hear your keystrokes. Enable Clicky in System Settings › Privacy & Security › Input Monitoring.";
    $("permission-action").textContent = "Open System Settings";
  }
}

volume.addEventListener("input", () => {
  setVolumeLabel(volume.value);
  invoke("set_volume", { volume: volume.value / 100 });
});
muted.addEventListener("change", () => invoke("set_muted", { muted: muted.checked }));
output.addEventListener("change", async () => {
  await invoke("set_output_device", { device: output.value || null });
  invoke("preview", { sound: "click" });
});
closeToTray.addEventListener("change", () => invoke("set_close_to_tray", { value: closeToTray.checked }));
startMinimized.addEventListener("change", () => invoke("set_start_minimized", { value: startMinimized.checked }));

document.querySelectorAll("[data-profile]").forEach((button) => {
  button.addEventListener("click", async () => {
    await invoke("set_profile", { profile: button.dataset.profile });
    invoke("preview", { sound: "click" });
  });
});

document.querySelectorAll("[data-sound]").forEach((button) => {
  button.addEventListener("click", () => invoke("preview", { sound: button.dataset.sound }));
});

$("permission-action").addEventListener("click", () =>
  invoke(permission === "relaunch" ? "relaunch" : "open_permission_settings"),
);
$("hide").addEventListener("click", () => invoke("hide_to_tray"));
$("sounds-folder").addEventListener("click", () => invoke("open_sounds_folder"));
$("quit").addEventListener("click", () => invoke("quit"));

listen("state-changed", (event) => render(event.payload));
invoke("get_state").then(render);
renderOutput();
refreshDevices();
window.addEventListener("focus", refreshDevices);
setInterval(() => {
  if (!document.hidden) refreshDevices();
}, 5000);
