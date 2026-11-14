/*
 * Pixel Chain - Milestone 0 u C-u
 * Generise Ed25519 kljuc, potpisuje i proverava poruku, racuna SHA-256.
 * Biblioteka: libsodium (https://libsodium.org)
 */
#include <stdio.h>
#include <string.h>
#include <sodium.h>

/* Ispisuje bajtove kao hex, mala slova (proverite sta kaze vasa specifikacija) */
static void print_hex(const char *label, const unsigned char *data, size_t len)
{
    char hex[2 * 128 + 1];
    sodium_bin2hex(hex, sizeof hex, data, len);
    printf("%s%s\n", label, hex);
}

/* Broji vodece nulte bitove u hesu (za proof of work) */
static int leading_zero_bits(const unsigned char *h, size_t len)
{
    int n = 0;
    for (size_t i = 0; i < len; i++) {
        if (h[i] == 0) { n += 8; continue; }
        for (int bit = 7; bit >= 0; bit--) {
            if (h[i] & (1u << bit)) return n;
            n++;
        }
    }
    return n;
}

int main(void)
{
    if (sodium_init() < 0) {
        fprintf(stderr, "Greska: libsodium nije mogao da se inicijalizuje\n");
        return 1;
    }

    /* 1. Generisanje para kljuceva */
    unsigned char pk[crypto_sign_PUBLICKEYBYTES];   /* 32 bajta - javni kljuc */
    unsigned char sk[crypto_sign_SECRETKEYBYTES];   /* 64 bajta - seed (32) + javni kljuc (32) */
    crypto_sign_keypair(pk, sk);
    print_hex("Javni kljuc:    ", pk, sizeof pk);
    print_hex("Privatni (seed): ", sk, 32);

    /* 2. Potpis i provera */
    const char *poruka = "zdravo";
    size_t len = strlen(poruka);
    unsigned char sig[crypto_sign_BYTES];           /* 64 bajta */
    crypto_sign_detached(sig, NULL, (const unsigned char *)poruka, len, sk);
    print_hex("Potpis:         ", sig, sizeof sig);

    int ok = crypto_sign_verify_detached(sig, (const unsigned char *)poruka, len, pk) == 0;
    printf("Potpis ispravan: %s\n", ok ? "DA" : "NE");

    /* Provera da izmenjena poruka NE prolazi */
    const char *lazna = "zdravO";
    int ok2 = crypto_sign_verify_detached(sig, (const unsigned char *)lazna, strlen(lazna), pk) == 0;
    printf("Izmenjena poruka prolazi: %s (treba NE)\n", ok2 ? "DA" : "NE");

    /* 3. SHA-256 */
    unsigned char hash[crypto_hash_sha256_BYTES];   /* 32 bajta */
    crypto_hash_sha256(hash, (const unsigned char *)poruka, len);
    print_hex("SHA-256:        ", hash, sizeof hash);
    printf("Vodecih nultih bitova: %d\n", leading_zero_bits(hash, sizeof hash));

    /* Kontrola: SHA-256 od "abc" mora biti
       ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad */
    crypto_hash_sha256(hash, (const unsigned char *)"abc", 3);
    print_hex("SHA-256(\"abc\"):  ", hash, sizeof hash);

    return 0;
}