// Shown over the website when the server no longer accepts this app version.
// There is deliberately no close button: the only choices are to update or to quit.
import { invoke } from "@tauri-apps/api/core";
import "./notice.css";

const update = document.getElementById("update") as HTMLButtonElement;
const quit = document.getElementById("quit") as HTMLButtonElement;

update.addEventListener("click", () => {
  update.disabled = true;
  quit.disabled = true;
  update.textContent = "Updating…";
  invoke("required_update", { action: "update" }).catch((e) => {
    console.error(e);
    update.disabled = false;
    quit.disabled = false;
    update.textContent = "Try again";
  });
});

quit.addEventListener("click", () => invoke("required_update", { action: "quit" }).catch(console.error));
