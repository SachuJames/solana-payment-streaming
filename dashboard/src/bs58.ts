const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

export function encode(data: Uint8Array): string {
  let num = 0n;
  for (const b of data) num = (num << 8n) | BigInt(b);
  let out = "";
  while (num > 0n) {
    out = ALPHABET[Number(num % 58n)] + out;
    num /= 58n;
  }
  for (const b of data) {
    if (b !== 0) break;
    out = "1" + out;
  }
  return out;
}

export function decode(s: string): Uint8Array {
  let num = 0n;
  for (const c of s) {
    const i = ALPHABET.indexOf(c);
    if (i < 0) throw new Error("invalid base58 character");
    num = num * 58n + BigInt(i);
  }
  const bytes: number[] = [];
  while (num > 0n) {
    bytes.unshift(Number(num & 0xffn));
    num >>= 8n;
  }
  let pad = 0;
  for (const c of s) {
    if (c !== "1") break;
    pad++;
  }
  return new Uint8Array([...new Array(pad).fill(0), ...bytes]);
}

export default { encode, decode };
