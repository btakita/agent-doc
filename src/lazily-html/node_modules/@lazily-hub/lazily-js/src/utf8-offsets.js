// -- UTF-8 wire offset conversion (#lzlosstree) -----------------------------
//
// The lossless-tree protocol carries leaf-local text offsets as **UTF-8 byte
// offsets** (lazily-spec § Offset policy); no binding may treat UTF-16 code
// units as wire offsets. JS strings are UTF-16, but this library's TextCrdt is
// **code-point granular** (`insertStr` iterates with `for…of`, one element per
// code point), so a wire byte offset converts to a code-point index here. That
// index doubles as `editLeaf`'s TextCrdt position and `splitLeaf`'s wire
// `at_char` (a Unicode scalar count, identical across bindings).
//
// A conversion returns `null` when the offset is out of range or does not land
// on a UTF-8 character boundary, so a bad offset fails closed.

/** UTF-8 byte length of the character whose code point is `cp`. */
function utf8Len(cp) {
  if (cp < 0x80) return 1;
  if (cp < 0x800) return 2;
  if (cp < 0x10000) return 3;
  return 4;
}

/**
 * UTF-8 byte offset `byte` into `s` → the number of Unicode scalars (code
 * points) before it, or `null` if out of range / not on a char boundary.
 */
export function byteToCodePoint(s, byte) {
  if (!Number.isInteger(byte) || byte < 0) return null;
  let b = 0;
  let cp = 0;
  for (const ch of s) {
    if (b === byte) return cp;
    b += utf8Len(ch.codePointAt(0));
    if (b > byte) return null; // offset falls inside this character
    cp += 1;
  }
  return b === byte ? cp : null;
}
