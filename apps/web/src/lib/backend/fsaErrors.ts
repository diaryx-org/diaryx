/**
 * Thrown when FSA initialization fails because it needs a user gesture
 * (e.g. to call showDirectoryPicker or requestPermission after browser restart).
 */
export class FsaGestureRequiredError extends Error {
  constructor(message = "Local folder access requires a click to reconnect.") {
    super(message);
    this.name = "FsaGestureRequiredError";
  }
}
