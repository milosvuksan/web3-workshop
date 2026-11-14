#include <openssl/evp.h>
#include <openssl/sha.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static double now(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}
int main(int argc, char **argv) {
    uint64_t n = argc > 1 ? strtoull(argv[1], NULL, 10) : 1000000;
    size_t size = argc > 2 ? strtoul(argv[2], NULL, 10) : 128;
    if (size < 8) return 1;
    unsigned char *buf = (unsigned char *)malloc(size), out[32];
    for (size_t j=0; j<size; ++j) buf[j] = j & 255;
#ifndef LOW_LEVEL
    EVP_MD *md = EVP_MD_fetch(NULL, "SHA256", NULL);
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    if (!md || !ctx) return 2;
#endif
    double start = now();
    for (uint64_t i=0; i<n; ++i) {
        for (int j=0; j<8; ++j) buf[size-8+j] = (unsigned char)(i >> (j*8));
#ifdef LOW_LEVEL
        SHA256_CTX ctx;
        SHA256_Init(&ctx); SHA256_Update(&ctx, buf, size); SHA256_Final(out, &ctx);
#else
        if (!EVP_DigestInit_ex2(ctx, md, NULL) || !EVP_DigestUpdate(ctx, buf, size) ||
            !EVP_DigestFinal_ex(ctx, out, NULL)) return 3;
#endif
    }
    double elapsed = now() - start;
    printf("{\"seconds\":%.9f,\"hashes_per_second\":%.2f,\"digest\":\"", elapsed, n/elapsed);
    for (int j=0; j<32; ++j) printf("%02x", out[j]);
    puts("\"}");
#ifndef LOW_LEVEL
    EVP_MD_CTX_free(ctx); EVP_MD_free(md);
#endif
    free(buf);
}
