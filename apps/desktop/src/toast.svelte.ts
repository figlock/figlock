/** One short message at the bottom of the window. */
class Toast {
  message = $state("");
  #timer: ReturnType<typeof setTimeout> | undefined;

  show(message: string) {
    this.message = message;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => (this.message = ""), 3000);
  }
}

export const toast = new Toast();
