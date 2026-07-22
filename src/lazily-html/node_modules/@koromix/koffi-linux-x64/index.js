let native = null;
let err = null;
if (native == null) {
  try {
    native = require("./linux_x64/koffi.node");
  } catch (e) {
    err = e;
  }
}
if (native == null) {
  try {
    native = require("./musl_x64/koffi.node");
  } catch (e) {
    err = e;
  }
}
if (native == null)
  throw err;
module.exports = native;
