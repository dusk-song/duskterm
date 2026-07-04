import { defineStore } from 'pinia';
import { ref } from 'vue';

export const useSecurityStore = defineStore('security', () => {
  const isLocked = ref(false);
  const hasPassword = ref(false);

  // Initial check
  const storedHash = localStorage.getItem('app-security-hash');
  const storedSalt = localStorage.getItem('app-security-salt');
  if (storedHash && storedSalt) {
    hasPassword.value = true;
    isLocked.value = true; // Lock by default on load if password exists
  }

  // Helper: Generate SHA-256 hash
  async function hashPassword(password, salt) {
    const enc = new TextEncoder();
    const keyMaterial = await window.crypto.subtle.importKey(
      "raw",
      enc.encode(password),
      { name: "PBKDF2" },
      false,
      ["deriveBits", "deriveKey"]
    );

    const saltBuffer = Uint8Array.from(atob(salt), c => c.charCodeAt(0));

    const key = await window.crypto.subtle.deriveKey(
      {
        name: "PBKDF2",
        salt: saltBuffer,
        iterations: 100000,
        hash: "SHA-256"
      },
      keyMaterial,
      { name: "AES-GCM", length: 256 },
      true,
      ["encrypt", "decrypt"]
    );

    // For verification, we export the key as JWK or raw format to string
    const exported = await window.crypto.subtle.exportKey("jwk", key);
    return exported.k;
  }

  async function setPassword(password) {
    if (!password) {
      // Remove password
      localStorage.removeItem('app-security-hash');
      localStorage.removeItem('app-security-salt');
      hasPassword.value = false;
      isLocked.value = false;
      return;
    }

    const salt = window.crypto.getRandomValues(new Uint8Array(16));
    const saltString = btoa(String.fromCharCode(...salt));

    const hash = await hashPassword(password, saltString);

    localStorage.setItem('app-security-salt', saltString);
    localStorage.setItem('app-security-hash', hash);
    hasPassword.value = true;
  }

  async function verifyPassword(password) {
    if (!hasPassword.value) return true;

    const salt = localStorage.getItem('app-security-salt');
    const hash = localStorage.getItem('app-security-hash');

    if (!salt || !hash) return true;

    const calculatedHash = await hashPassword(password, salt);
    return calculatedHash === hash;
  }

  async function unlock(password) {
    if (await verifyPassword(password)) {
      isLocked.value = false;
      return true;
    }
    return false;
  }

  return {
    isLocked,
    hasPassword,
    setPassword,
    verifyPassword,
    unlock
  };
});
