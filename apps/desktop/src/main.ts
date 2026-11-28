import "@figlock/ui/app.css";
import { mount } from "svelte";

import App from "./App.svelte";
import { applyAppearance, savedAppearance } from "./appearance";

applyAppearance(savedAppearance());
mount(App, { target: document.getElementById("app")! });
