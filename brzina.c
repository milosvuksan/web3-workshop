/*
 * Merenje brzine heširanja (heševa u sekundi) za izbor tezine.
 * Simulira rudarenje: hesira blok od 128 bajtova i menja nonce u svakom pokusaju.
 * Kompajliranje: gcc brzina.c -o brzina.exe -O2 -static -lsodium
 */
#include <stdio.h>
#include <string.h>
#include <stdint.h>
#include <time.h>
#include <math.h>
#include <sodium.h>

#define TRAJANJE_SEKUNDI 5.0

int main(void)
{
    if (sodium_init() < 0) {
        fprintf(stderr, "Greska: libsodium nije mogao da se inicijalizuje\n");
        return 1;
    }

    unsigned char blok[128];
    unsigned char hash[crypto_hash_sha256_BYTES];
    memset(blok, 0xAB, sizeof blok);           /* neki sadrzaj bloka */

    printf("Merim %.0f sekundi...\n", TRAJANJE_SEKUNDI);

    uint64_t nonce = 0;
    clock_t start = clock();
    double proteklo = 0.0;

    while (proteklo < TRAJANJE_SEKUNDI) {
        /* 100.000 pokusaja, pa proveri vreme (da merenje vremena ne usporava) */
        for (int i = 0; i < 100000; i++) {
            memcpy(blok + 120, &nonce, 8);     /* nonce u poslednjih 8 bajtova */
            crypto_hash_sha256(hash, blok, sizeof blok);
            nonce++;
        }
        proteklo = (double)(clock() - start) / CLOCKS_PER_SEC;
    }

    double brzina = nonce / proteklo;
    printf("\nUradjeno %llu heseva za %.2f s\n", (unsigned long long)nonce, proteklo);
    printf("Brzina: %.0f heseva u sekundi\n\n", brzina);

    printf("Koliko bi SAM ovaj racunar u proseku trazio blok:\n");
    for (int d = 14; d <= 26; d++) {
        printf("  tezina %2d bita: %8.1f s\n", d, pow(2.0, d) / brzina);
    }
    return 0;
}