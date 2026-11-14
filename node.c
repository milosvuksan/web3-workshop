/*
 * Pixel Chain - samostalan cvor (milestone 2), C + libsodium
 *
 * Kompajliranje:  gcc node.c -o node.exe -O2 -static -lsodium
 * Pokretanje:     node.exe                 (samoprovera, pa rudarenje zauvek)
 *                 node.exe --vectors       (ispise test podatke za SPEC.md)
 *                 node.exe --diff 18       (manja tezina, samo za lokalno testiranje)
 *
 * Svi formati su opisani u SPEC.md. Ukratko:
 *   - Ed25519 potpisi, SHA-256 hes, hex mala slova, brojevi big-endian
 *   - transakcija: 78 bajtova tela + 64 bajta potpisa
 *   - zaglavlje bloka: 122 bajta, hes = SHA-256(zaglavlje)
 *   - platno 64x32 = 2048 parcela, tezina 23 bita
 */
#define _CRT_SECURE_NO_WARNINGS
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>
#include <sodium.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#ifndef ENABLE_VIRTUAL_TERMINAL_PROCESSING
#define ENABLE_VIRTUAL_TERMINAL_PROCESSING 0x0004
#endif
#endif

/* ===================== Konstante iz specifikacije ===================== */

#define CANVAS_W      64
#define CANVAS_H      32
#define NPARCELS      (CANVAS_W * CANVAS_H)   /* 2048 */
#define DIFFICULTY    23                      /* vodecih nultih bitova */
#define MAX_TX        100                     /* najvise transakcija u bloku */

#define TX_PAINT      1
#define TX_TRANSFER   2

#define TX_BODY_LEN   78                      /* bajtovi koji se potpisuju */
#define TX_FULL_LEN   (TX_BODY_LEN + 64)      /* telo + potpis = 142 */
#define HEADER_LEN    122                     /* bajtovi koji se hesuju */
#define NONCE_OFFSET  114                     /* gde je nonce u zaglavlju */

static int g_difficulty = DIFFICULTY;

/* ===================== Strukture ===================== */

typedef struct {
    uint8_t  type;          /* 1 = bojenje, 2 = prenos */
    uint16_t parcel;        /* 0..2047 */
    uint64_t nonce;         /* mora biti = brojac parcele + 1 */
    uint8_t  signer[32];    /* javni kljuc potpisnika (trenutni vlasnik) */
    uint8_t  color[3];      /* RGB; kod prenosa mora biti 000000 */
    uint8_t  to[32];        /* novi vlasnik; kod bojenja moraju biti nule */
    uint8_t  sig[64];       /* Ed25519 potpis nad 78 bajtova tela */
} Tx;

typedef struct {
    uint64_t height;
    uint8_t  prev[32];
    uint64_t time;          /* unix vreme u sekundama */
    uint8_t  miner[32];
    uint16_t ntx;
    uint8_t  txroot[32];    /* SHA-256 svih transakcija (142 bajta svaka), redom */
    uint64_t nonce;
    Tx      *tx;            /* ntx transakcija */
    uint8_t  hash[32];      /* SHA-256(zaglavlje) */
    int      parent;        /* indeks roditelja u g_blocks (-1 za genesis) */
    int      reward;        /* parcela koju je rudar dobio, -1 ako nijednu */
} Block;

typedef struct {
    int      owned;
    uint8_t  owner[32];
    uint8_t  color[3];
    uint64_t seq;           /* broj primenjenih transakcija nad parcelom */
} Parcel;

typedef struct {
    Parcel p[NPARCELS];
} State;

typedef struct {
    uint8_t seed[32];
    uint8_t pk[32];
    uint8_t sk[64];
} Key;

/* ===================== Pomocne funkcije ===================== */

static void put_u16(uint8_t *b, uint16_t v) { b[0] = v >> 8; b[1] = v & 0xff; }
static void put_u64(uint8_t *b, uint64_t v)
{
    for (int i = 7; i >= 0; i--) { b[i] = v & 0xff; v >>= 8; }
}

static void to_hex(char *out, const uint8_t *in, size_t len)
{
    sodium_bin2hex(out, len * 2 + 1, in, len);
}

static int is_zero(const uint8_t *b, size_t len)
{
    for (size_t i = 0; i < len; i++) if (b[i]) return 0;
    return 1;
}

static int leading_zero_bits(const uint8_t *h)
{
    int n = 0;
    for (int i = 0; i < 32; i++) {
        if (h[i] == 0) { n += 8; continue; }
        for (int bit = 7; bit >= 0; bit--) {
            if (h[i] & (1u << bit)) return n;
            n++;
        }
    }
    return n;
}

static void sha256(uint8_t out[32], const uint8_t *in, size_t len)
{
    crypto_hash_sha256(out, in, len);
}

/* ===================== Kljucevi ===================== */

static void key_from_seed(Key *k, const uint8_t seed[32])
{
    memcpy(k->seed, seed, 32);
    crypto_sign_seed_keypair(k->pk, k->sk, seed);
}

/* Ucita seed iz fajla (hex) ili napravi novi i sacuva ga. */
static void key_load_or_create(Key *k, const char *path)
{
    uint8_t seed[32];
    char line[128] = {0};
    FILE *f = fopen(path, "r");
    if (f && fgets(line, sizeof line, f)) {
        size_t n = 0;
        line[strcspn(line, "\r\n")] = 0;
        if (sodium_hex2bin(seed, 32, line, strlen(line), NULL, &n, NULL) == 0 && n == 32) {
            fclose(f);
            key_from_seed(k, seed);
            return;
        }
    }
    if (f) fclose(f);
    randombytes_buf(seed, 32);
    key_from_seed(k, seed);
    f = fopen(path, "w");
    if (f) {
        char hex[65];
        to_hex(hex, seed, 32);
        fprintf(f, "%s\n", hex);
        fclose(f);
    }
}

/* ===================== Transakcije ===================== */

/* Telo transakcije (78 bajtova), nad njim se racuna potpis:
 *   type(1) | parcel(2) | nonce(8) | signer(32) | color(3) | to(32)          */
static void tx_body(const Tx *t, uint8_t out[TX_BODY_LEN])
{
    out[0] = t->type;
    put_u16(out + 1, t->parcel);
    put_u64(out + 3, t->nonce);
    memcpy(out + 11, t->signer, 32);
    memcpy(out + 43, t->color, 3);
    memcpy(out + 46, t->to, 32);
}

/* Cela transakcija (142 bajta): telo + potpis */
static void tx_full(const Tx *t, uint8_t out[TX_FULL_LEN])
{
    tx_body(t, out);
    memcpy(out + TX_BODY_LEN, t->sig, 64);
}

static void tx_sign(Tx *t, const Key *k)
{
    uint8_t body[TX_BODY_LEN];
    memcpy(t->signer, k->pk, 32);
    tx_body(t, body);
    crypto_sign_detached(t->sig, NULL, body, TX_BODY_LEN, k->sk);
}

static void tx_id(const Tx *t, uint8_t out[32])
{
    uint8_t full[TX_FULL_LEN];
    tx_full(t, full);
    sha256(out, full, TX_FULL_LEN);
}

static Tx make_paint(const Key *k, uint16_t parcel, uint64_t nonce, uint8_t r, uint8_t g, uint8_t b)
{
    Tx t;
    memset(&t, 0, sizeof t);
    t.type = TX_PAINT; t.parcel = parcel; t.nonce = nonce;
    t.color[0] = r; t.color[1] = g; t.color[2] = b;
    tx_sign(&t, k);
    return t;
}

static Tx make_transfer(const Key *k, uint16_t parcel, uint64_t nonce, const uint8_t to[32])
{
    Tx t;
    memset(&t, 0, sizeof t);
    t.type = TX_TRANSFER; t.parcel = parcel; t.nonce = nonce;
    memcpy(t.to, to, 32);
    tx_sign(&t, k);
    return t;
}

/* Proveri i primeni transakciju na stanje. Vraca 0 ako je ispravna, inace kod greske.
 * Redosled provera je deo specifikacije. */
static int tx_apply(State *st, const Tx *t)
{
    uint8_t body[TX_BODY_LEN];
    if (t->type != TX_PAINT && t->type != TX_TRANSFER) return 1;            /* nepoznat tip */
    if (t->parcel >= NPARCELS) return 2;                                      /* nema parcele */
    if (t->type == TX_PAINT && !is_zero(t->to, 32)) return 3;                 /* polje to mora biti 0 */
    if (t->type == TX_TRANSFER && !is_zero(t->color, 3)) return 3;            /* boja mora biti 0 */
    tx_body(t, body);
    if (crypto_sign_verify_detached(t->sig, body, TX_BODY_LEN, t->signer) != 0) return 4; /* potpis */
    Parcel *p = &st->p[t->parcel];
    if (!p->owned || memcmp(p->owner, t->signer, 32) != 0) return 5;          /* nije vlasnik */
    if (t->nonce != p->seq + 1) return 6;                                     /* ponovljena / pogresan redosled */

    if (t->type == TX_PAINT) memcpy(p->color, t->color, 3);
    else                     memcpy(p->owner, t->to, 32);
    p->seq++;
    return 0;
}

/* ===================== Blokovi ===================== */

/* Zaglavlje bloka (122 bajta), nad njim se racuna hes:
 *   height(8) | prev(32) | time(8) | miner(32) | ntx(2) | txroot(32) | nonce(8)  */
static void block_header(const Block *b, uint8_t out[HEADER_LEN])
{
    put_u64(out + 0, b->height);
    memcpy(out + 8, b->prev, 32);
    put_u64(out + 40, b->time);
    memcpy(out + 48, b->miner, 32);
    put_u16(out + 80, b->ntx);
    memcpy(out + 82, b->txroot, 32);
    put_u64(out + NONCE_OFFSET, b->nonce);
}

static void block_hash(const Block *b, uint8_t out[32])
{
    uint8_t hdr[HEADER_LEN];
    block_header(b, hdr);
    sha256(out, hdr, HEADER_LEN);
}

/* txroot = SHA-256(tx1 || tx2 || ... ), svaka po 142 bajta; za 0 tx = SHA-256 praznog niza */
static void compute_txroot(const Tx *txs, int n, uint8_t out[32])
{
    crypto_hash_sha256_state s;
    uint8_t full[TX_FULL_LEN];
    crypto_hash_sha256_init(&s);
    for (int i = 0; i < n; i++) {
        tx_full(&txs[i], full);
        crypto_hash_sha256_update(&s, full, TX_FULL_LEN);
    }
    crypto_hash_sha256_final(&s, out);
}

/* Nagrada: prva parcela iz poslednja 2 bajta hesa; ako je zauzeta,
 * sledeci pokusaj p = (p + korak) mod 2048, korak = (time OR 1) mod 2048 (uvek neparan,
 * pa se prodje kroz svih 2048 parcela). Ako su sve zauzete, nema nagrade. */
static int reward_parcel(const State *st, const Block *b)
{
    uint32_t p = (((uint32_t)b->hash[30] << 8) | b->hash[31]) % NPARCELS;
    uint32_t step = (uint32_t)((b->time | 1) % NPARCELS);
    for (int i = 0; i < NPARCELS; i++) {
        if (!st->p[p].owned) return (int)p;
        p = (p + step) % NPARCELS;
    }
    return -1;
}

/* Primeni blok na stanje: prvo transakcije redom, pa nagrada rudaru.
 * Vraca 0 ako je sve ispravno. */
static int block_apply(State *st, Block *b)
{
    if (b->height == 0) { b->reward = -1; return 0; }   /* genesis ne menja platno */
    for (int i = 0; i < b->ntx; i++) {
        if (tx_apply(st, &b->tx[i]) != 0) return -1;
    }
    int p = reward_parcel(st, b);
    if (p >= 0) {
        Parcel *pp = &st->p[p];
        pp->owned = 1;
        memcpy(pp->owner, b->miner, 32);
        pp->color[0] = pp->color[1] = pp->color[2] = 0xff;   /* nova parcela je bela */
    }
    b->reward = p;
    return 0;
}

/* ===================== Lanac ===================== */

static Block *g_blocks = NULL;
static int    g_nblocks = 0, g_cap = 0;
static int    g_tip = 0;

static void make_genesis(Block *g)
{
    memset(g, 0, sizeof *g);
    compute_txroot(NULL, 0, g->txroot);
    block_hash(g, g->hash);
    g->parent = -1;
    g->reward = -1;
}

static int find_block(const uint8_t hash[32])
{
    for (int i = g_nblocks - 1; i >= 0; i--)
        if (memcmp(g_blocks[i].hash, hash, 32) == 0) return i;
    return -1;
}

/* Stanje platna posle bloka idx: primeni sve blokove od genesis-a do njega. */
static void state_at(int idx, State *st)
{
    static int path[1 << 20];
    int n = 0;
    for (int i = idx; i >= 0; i = g_blocks[i].parent) path[n++] = i;
    memset(st, 0, sizeof *st);
    for (int i = n - 1; i >= 0; i--) block_apply(st, &g_blocks[path[i]]);
}

/* Da li je lanac sa vrhom a bolji od lanca sa vrhom b?
 * Tezina je fiksna, pa vise rada = veca visina. Izjednacenje: leksikografski manji hes. */
static int better_tip(int a, int b)
{
    if (g_blocks[a].height != g_blocks[b].height) return g_blocks[a].height > g_blocks[b].height;
    return memcmp(g_blocks[a].hash, g_blocks[b].hash, 32) < 0;
}

/* Proveri blok i dodaj ga u lanac. Vraca 0 ako je dodat. */
static int add_block(const Block *in, const char **why)
{
    uint8_t h[32], root[32];
    *why = "ok";
    block_hash(in, h);
    if (memcmp(h, in->hash, 32) != 0)          { *why = "hes se ne poklapa"; return -1; }
    if (find_block(in->hash) >= 0)             { *why = "vec postoji"; return -1; }
    int parent = find_block(in->prev);
    if (parent < 0)                            { *why = "nepoznat prethodni blok"; return -1; }
    const Block *pb = &g_blocks[parent];
    if (in->height != pb->height + 1)          { *why = "pogresna visina"; return -1; }
    if (in->time < pb->time)                   { *why = "vreme manje od prethodnog bloka"; return -1; }
    if (in->ntx > MAX_TX)                      { *why = "previse transakcija"; return -1; }
    compute_txroot(in->tx, in->ntx, root);
    if (memcmp(root, in->txroot, 32) != 0)     { *why = "txroot se ne poklapa"; return -1; }
    if (leading_zero_bits(in->hash) < g_difficulty) { *why = "nedovoljno rada"; return -1; }

    static State st;
    state_at(parent, &st);
    Block b = *in;
    b.tx = NULL;
    if (b.ntx > 0) {
        b.tx = malloc(sizeof(Tx) * b.ntx);
        memcpy(b.tx, in->tx, sizeof(Tx) * b.ntx);
    }
    b.parent = parent;
    if (block_apply(&st, &b) != 0) { free(b.tx); *why = "neispravna transakcija"; return -1; }

    if (g_nblocks == g_cap) {
        g_cap = g_cap ? g_cap * 2 : 64;
        g_blocks = realloc(g_blocks, sizeof(Block) * g_cap);
    }
    g_blocks[g_nblocks++] = b;
    if (better_tip(g_nblocks - 1, g_tip)) g_tip = g_nblocks - 1;
    return 0;
}

static void chain_init(void)
{
    Block g;
    make_genesis(&g);
    g_cap = 64;
    g_blocks = malloc(sizeof(Block) * g_cap);
    g_blocks[0] = g;
    g_nblocks = 1;
    g_tip = 0;
}

/* ===================== JSON (format poruka na mrezi) ===================== */

static int tx_to_json(const Tx *t, char *out, size_t cap)
{
    char signer[65], to[65], sig[129], color[7];
    to_hex(signer, t->signer, 32);
    to_hex(to, t->to, 32);
    to_hex(sig, t->sig, 64);
    to_hex(color, t->color, 3);
    return snprintf(out, cap,
        "{\"type\":\"%s\",\"parcel\":%u,\"nonce\":%llu,\"signer\":\"%s\",\"color\":\"%s\",\"to\":\"%s\",\"sig\":\"%s\"}",
        t->type == TX_PAINT ? "paint" : "transfer", t->parcel, (unsigned long long)t->nonce,
        signer, color, to, sig);
}

static void block_to_json(const Block *b, char *out, size_t cap)
{
    char prev[65], miner[65], root[65], hash[65];
    to_hex(prev, b->prev, 32);
    to_hex(miner, b->miner, 32);
    to_hex(root, b->txroot, 32);
    to_hex(hash, b->hash, 32);
    int n = snprintf(out, cap,
        "{\"height\":%llu,\"prev\":\"%s\",\"time\":%llu,\"miner\":\"%s\",\"txroot\":\"%s\",\"nonce\":%llu,\"hash\":\"%s\",\"txs\":[",
        (unsigned long long)b->height, prev, (unsigned long long)b->time, miner, root,
        (unsigned long long)b->nonce, hash);
    for (int i = 0; i < b->ntx && n < (int)cap; i++) {
        if (i) n += snprintf(out + n, cap - n, ",");
        n += tx_to_json(&b->tx[i], out + n, cap - n);
    }
    if (n < (int)cap) snprintf(out + n, cap - n, "]}");
}

/* ===================== Prikaz platna ===================== */

static void parcel_rgb(const State *st, int p, uint8_t rgb[3])
{
    if (!st->p[p].owned) { rgb[0] = rgb[1] = rgb[2] = 28; return; }   /* niciji: tamno sivo */
    memcpy(rgb, st->p[p].color, 3);
}

/* Konzola: dve vrste piksela po redu teksta (znak "gornja polovina bloka") */
static void draw_console(const State *st)
{
    for (int y = 0; y < CANVAS_H; y += 2) {
        for (int x = 0; x < CANVAS_W; x++) {
            uint8_t a[3], b[3];
            parcel_rgb(st, y * CANVAS_W + x, a);
            parcel_rgb(st, (y + 1) * CANVAS_W + x, b);
            printf("\x1b[38;2;%d;%d;%dm\x1b[48;2;%d;%d;%dm\xe2\x96\x80",
                   a[0], a[1], a[2], b[0], b[1], b[2]);
        }
        printf("\x1b[0m\n");
    }
}

/* Slika: canvas.bmp, svaka parcela je kvadrat 10x10 piksela */
static void save_bmp(const State *st, const char *path)
{
    const int S = 10, W = CANVAS_W * S, H = CANVAS_H * S;
    const int row = W * 3;                         /* 640*3 = 1920, deljivo sa 4 */
    const uint32_t data = row * H, size = 54 + data;
    uint8_t hdr[54] = {0};
    hdr[0] = 'B'; hdr[1] = 'M';
    memcpy(hdr + 2, &size, 4);
    hdr[10] = 54; hdr[14] = 40;
    memcpy(hdr + 18, &W, 4);
    memcpy(hdr + 22, &H, 4);
    hdr[26] = 1; hdr[28] = 24;
    memcpy(hdr + 34, &data, 4);

    FILE *f = fopen(path, "wb");
    if (!f) return;
    fwrite(hdr, 1, 54, f);
    uint8_t *line = malloc(row);
    for (int py = H - 1; py >= 0; py--) {           /* BMP ide odozdo nagore */
        for (int px = 0; px < W; px++) {
            uint8_t c[3];
            parcel_rgb(st, (py / S) * CANVAS_W + px / S, c);
            line[px * 3 + 0] = c[2];               /* BMP je BGR */
            line[px * 3 + 1] = c[1];
            line[px * 3 + 2] = c[0];
        }
        fwrite(line, 1, row, f);
    }
    free(line);
    fclose(f);
}

/* ===================== Samoprovera ===================== */

static int g_fail = 0;
static void check(int ok, const char *what)
{
    printf("  [%s] %s\n", ok ? " OK " : "GRESKA", what);
    if (!ok) g_fail++;
}

static int self_test(void)
{
    uint8_t h[32], seed[32];
    char hex[129];
    printf("Samoprovera:\n");

    sha256(h, (const uint8_t *)"abc", 3);
    to_hex(hex, h, 32);
    check(strcmp(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad") == 0,
          "SHA-256(\"abc\")");

    Key rfc;
    sodium_hex2bin(seed, 32, "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
                   64, NULL, NULL, NULL);
    key_from_seed(&rfc, seed);
    to_hex(hex, rfc.pk, 32);
    check(strcmp(hex, "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a") == 0,
          "Ed25519 javni kljuc (RFC 8032 test 1)");

    /* Pravila transakcija na malom stanju: A poseduje parcelu 5 */
    Key A, B;
    memset(seed, 1, 32); key_from_seed(&A, seed);
    memset(seed, 2, 32); key_from_seed(&B, seed);
    static State st;
    memset(&st, 0, sizeof st);
    st.p[5].owned = 1;
    memcpy(st.p[5].owner, A.pk, 32);

    Tx t1 = make_paint(&A, 5, 1, 255, 0, 0);
    check(tx_apply(&st, &t1) == 0, "vlasnik boji svoju parcelu");
    check(tx_apply(&st, &t1) == 6, "ista transakcija drugi put se odbija (ponavljanje)");
    Tx t2 = make_paint(&B, 5, 2, 0, 0, 255);
    check(tx_apply(&st, &t2) == 5, "tudja parcela se ne moze bojiti");
    Tx t3 = make_paint(&A, 5, 2, 0, 255, 0);
    t3.color[0] = 9;                                /* izmena posle potpisa */
    check(tx_apply(&st, &t3) == 4, "izmenjena transakcija ima neispravan potpis");
    Tx t4 = make_transfer(&A, 5, 2, B.pk);
    check(tx_apply(&st, &t4) == 0, "prenos A -> B");
    Tx t5 = make_paint(&A, 5, 3, 1, 2, 3);
    check(tx_apply(&st, &t5) == 5, "posle prenosa A vise ne moze da boji");
    Tx t6 = make_paint(&B, 5, 3, 0, 0, 255);
    check(tx_apply(&st, &t6) == 0, "novi vlasnik B boji");

    /* Nagrada kad je parcela zauzeta prolazi kroz sve parcele */
    static State full;
    memset(&full, 0, sizeof full);
    for (int i = 0; i < NPARCELS; i++) full.p[i].owned = 1;
    full.p[1234].owned = 0;
    Block fb;
    memset(&fb, 0, sizeof fb);
    fb.time = 1763100000;
    fb.hash[30] = 0x0e; fb.hash[31] = 0xc8;
    check(reward_parcel(&full, &fb) == 1234, "zauzeta parcela: nadje jedinu slobodnu");
    full.p[1234].owned = 1;
    check(reward_parcel(&full, &fb) == -1, "sve zauzete: nema nagrade");

    printf("%s\n\n", g_fail ? "Samoprovera NIJE prosla!" : "Samoprovera prosla.");
    return g_fail;
}

/* ===================== Test podaci za SPEC.md ===================== */

static void mine(Block *b);

static void print_vectors(void)
{
    uint8_t seed[32], buf[TX_FULL_LEN], id[32];
    char hex[2 * TX_FULL_LEN + 1];
    Key A, B;
    memset(seed, 1, 32); key_from_seed(&A, seed);
    memset(seed, 2, 32); key_from_seed(&B, seed);

    printf("== Test podaci (tezina %d) ==\n\n", g_difficulty);
    to_hex(hex, A.seed, 32); printf("Kljuc A seed:   %s\n", hex);
    to_hex(hex, A.pk, 32);   printf("Kljuc A javni:  %s\n", hex);
    to_hex(hex, B.seed, 32); printf("Kljuc B seed:   %s\n", hex);
    to_hex(hex, B.pk, 32);   printf("Kljuc B javni:  %s\n\n", hex);

    Block g;
    make_genesis(&g);
    uint8_t hdr[HEADER_LEN];
    block_header(&g, hdr);
    to_hex(hex, hdr, HEADER_LEN); printf("Genesis zaglavlje: %s\n", hex);
    to_hex(hex, g.hash, 32);      printf("Genesis hes:       %s\n\n", hex);

    Tx p = make_paint(&A, 1736, 1, 0xff, 0x00, 0x00);
    tx_body(&p, buf);  to_hex(hex, buf, TX_BODY_LEN); printf("Bojenje (A, parcela 1736, nonce 1, ff0000) telo:\n  %s\n", hex);
    to_hex(hex, p.sig, 64); printf("  potpis: %s\n", hex);
    tx_id(&p, id);     to_hex(hex, id, 32);           printf("  id:     %s\n\n", hex);

    Tx t = make_transfer(&A, 1736, 2, B.pk);
    tx_body(&t, buf);  to_hex(hex, buf, TX_BODY_LEN); printf("Prenos (A -> B, parcela 1736, nonce 2) telo:\n  %s\n", hex);
    to_hex(hex, t.sig, 64); printf("  potpis: %s\n", hex);
    tx_id(&t, id);     to_hex(hex, id, 32);           printf("  id:     %s\n\n", hex);

    Block b;
    memset(&b, 0, sizeof b);
    b.height = 1;
    memcpy(b.prev, g.hash, 32);
    b.time = 1763100000;
    memcpy(b.miner, A.pk, 32);
    compute_txroot(NULL, 0, b.txroot);
    printf("Rudarim blok 1 (rudar A, time 1763100000, bez transakcija)...\n");
    mine(&b);
    block_header(&b, hdr);
    to_hex(hex, hdr, HEADER_LEN); printf("Blok 1 zaglavlje: %s\n", hex);
    printf("Blok 1 nonce:     %llu\n", (unsigned long long)b.nonce);
    to_hex(hex, b.hash, 32);      printf("Blok 1 hes:       %s\n", hex);
    static State st;
    memset(&st, 0, sizeof st);
    int r = reward_parcel(&st, &b);
    printf("Blok 1 parcela:   %d (x=%d, y=%d)\n", r, r % CANVAS_W, r / CANVAS_W);
    st.p[r].owned = 1;
    int r2 = reward_parcel(&st, &b);
    printf("Ako je %d zauzeta: %d (korak %u)\n", r, r2, (unsigned)((b.time | 1) % NPARCELS));
}

/* ===================== Rudarenje ===================== */

static double g_hashrate = 0;       /* izmerena brzina (heseva u sekundi) */
static double g_last_secs = 0;      /* koliko je trajalo rudarenje poslednjeg bloka */
static double g_total_secs = 0;     /* ukupno vreme rudarenja svih blokova */
static long   g_mined = 0;          /* koliko je blokova izrudareno */

/* Ocekivano vreme za jedan blok: u proseku treba 2^tezina pokusaja */
static double expected_secs(void)
{
    if (g_hashrate <= 0) return 0;
    return (double)(1ULL << g_difficulty) / g_hashrate;
}

/* Trazi nonce dok hes nema dovoljno nultih bitova.
 * Otprilike svake sekunde ispise koliko traje, brzinu i ocekivano vreme. */
static void mine(Block *b)
{
    uint8_t hdr[HEADER_LEN], h[32];
    block_header(b, hdr);
    uint64_t nonce = 0, tries = 0;
    clock_t start = clock(), last_print = start;
    for (;;) {
        put_u64(hdr + NONCE_OFFSET, nonce);
        sha256(h, hdr, HEADER_LEN);
        tries++;
        if (leading_zero_bits(h) >= g_difficulty) break;
        nonce++;

        if ((tries & 0xFFFF) == 0) {                     /* na svakih 65.536 pokusaja */
            clock_t now = clock();
            if (now - last_print >= CLOCKS_PER_SEC) {
                double sec = (double)(now - start) / CLOCKS_PER_SEC;
                double rate = tries / sec;
                double expect = (double)(1ULL << g_difficulty) / rate;
                printf("\r  rudarim %5.1f s | %.2f mil. pokusaja | %.0f heseva/s | ocekivano ~%.1f s po bloku   ",
                       sec, tries / 1e6, rate, expect);
                fflush(stdout);
                last_print = now;
            }
        }
    }
    double sec = (double)(clock() - start) / CLOCKS_PER_SEC;
    if (sec > 0.01) g_hashrate = tries / sec;
    g_last_secs = sec;
    g_total_secs += sec;
    g_mined++;
    printf("\r  nadjen blok za %.1f s (%.2f mil. pokusaja)%40s\n", sec, tries / 1e6, "");
    b->nonce = nonce;
    memcpy(b->hash, h, 32);
}

/* ===================== Demo novcanik: sam pravi bojenja i prenose ===================== */

static Key g_keys[2];          /* A = rudar, B = druga adresa za prenose */
static Tx  g_mempool[MAX_TX];
static int g_nmem = 0;
static char g_log[8][160];
static int g_nlog = 0;

static void logf_line(const char *s)
{
    if (g_nlog == 8) { memmove(g_log[0], g_log[1], sizeof g_log[0] * 7); g_nlog = 7; }
    snprintf(g_log[g_nlog++], sizeof g_log[0], "%s", s);
}

static int key_index(const uint8_t pk[32])
{
    for (int i = 0; i < 2; i++) if (memcmp(g_keys[i].pk, pk, 32) == 0) return i;
    return -1;
}

static int in_mempool(uint16_t parcel)
{
    for (int i = 0; i < g_nmem; i++) if (g_mempool[i].parcel == parcel) return 1;
    return 0;
}

/* Posle svakog bloka: oboji 1-3 nasumicne svoje parcele, ponekad prenesi jednu na drugi kljuc */
static void demo_transactions(const State *st)
{
    int mine_parcels[NPARCELS], n = 0;
    for (int i = 0; i < NPARCELS; i++)
        if (st->p[i].owned && key_index(st->p[i].owner) >= 0 && !in_mempool(i)) mine_parcels[n++] = i;
    if (n == 0) return;

    int count = 1 + randombytes_uniform(3);
    for (int c = 0; c < count && g_nmem < MAX_TX && n > 0; c++) {
        int j = randombytes_uniform(n);
        int p = mine_parcels[j];
        mine_parcels[j] = mine_parcels[--n];
        const Key *k = &g_keys[key_index(st->p[p].owner)];
        char line[160];
        if (randombytes_uniform(5) == 0) {
            const Key *other = &g_keys[k == &g_keys[0] ? 1 : 0];
            g_mempool[g_nmem++] = make_transfer(k, p, st->p[p].seq + 1, other->pk);
            snprintf(line, sizeof line, "tx: prenos parcele %d (%d,%d) na kljuc %c",
                     p, p % CANVAS_W, p / CANVAS_W, other == &g_keys[0] ? 'A' : 'B');
        } else {
            uint8_t rgb[3];
            randombytes_buf(rgb, 3);
            g_mempool[g_nmem++] = make_paint(k, p, st->p[p].seq + 1, rgb[0], rgb[1], rgb[2]);
            snprintf(line, sizeof line, "tx: bojenje parcele %d (%d,%d) u #%02x%02x%02x",
                     p, p % CANVAS_W, p / CANVAS_W, rgb[0], rgb[1], rgb[2]);
        }
        logf_line(line);
    }
}

/* Uzmi iz mempool-a transakcije koje su ispravne nad stanjem vrha, redom. */
static int select_txs(const State *tip_state, Tx *out)
{
    static State tmp;
    tmp = *tip_state;
    int n = 0;
    for (int i = 0; i < g_nmem && n < MAX_TX; i++) {
        if (tx_apply(&tmp, &g_mempool[i]) == 0) out[n++] = g_mempool[i];
    }
    return n;
}

/* Izbaci iz mempool-a sve sto vise nije ispravno nad novim vrhom (ukljucene i zastarele). */
static void prune_mempool(const State *tip_state)
{
    static State tmp;
    tmp = *tip_state;
    int k = 0;
    for (int i = 0; i < g_nmem; i++)
        if (tx_apply(&tmp, &g_mempool[i]) == 0) g_mempool[k++] = g_mempool[i];
    g_nmem = k;
}

/* ===================== Ekran ===================== */

static void draw_screen(const State *st)
{
    const Block *tip = &g_blocks[g_tip];
    char hex[65], pk[65];
    int owned = 0, mine_a = 0, mine_b = 0;
    for (int i = 0; i < NPARCELS; i++) {
        if (!st->p[i].owned) continue;
        owned++;
        if (memcmp(st->p[i].owner, g_keys[0].pk, 32) == 0) mine_a++;
        if (memcmp(st->p[i].owner, g_keys[1].pk, 32) == 0) mine_b++;
    }
    printf("\x1b[H\x1b[2J");
    printf("Pixel Chain - samostalan cvor   tezina %d bita   %.0f heseva/s\n", g_difficulty, g_hashrate);
    to_hex(pk, g_keys[0].pk, 32);
    printf("Moj kljuc A: %.16s...\n", pk);
    to_hex(hex, tip->hash, 32);
    printf("Visina %llu   hes %.24s...   blokova u memoriji %d\n",
           (unsigned long long)tip->height, hex, g_nblocks);
    printf("Zauzeto %d/%d parcela   (kljuc A: %d, kljuc B: %d)\n", owned, NPARCELS, mine_a, mine_b);
    if (g_mined > 0)
        printf("Vreme po bloku: poslednji %.1f s | prosek %.1f s (%ld blokova) | ocekivano ~%.1f s\n\n",
               g_last_secs, g_total_secs / g_mined, g_mined, expected_secs());
    else
        printf("Vreme po bloku: jos nema izrudarenih blokova\n\n");
    draw_console(st);
    printf("\n");
    for (int i = 0; i < g_nlog; i++) printf("%s\n", g_log[i]);
    fflush(stdout);
}

/* ===================== main ===================== */

int main(int argc, char **argv)
{
#ifdef _WIN32
    SetConsoleOutputCP(CP_UTF8);
    HANDLE hout = GetStdHandle(STD_OUTPUT_HANDLE);
    DWORD mode;
    if (GetConsoleMode(hout, &mode)) SetConsoleMode(hout, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
#endif
    if (sodium_init() < 0) { fprintf(stderr, "libsodium nije pokrenut\n"); return 1; }

    int vectors = 0;
    long max_blocks = -1;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--vectors") == 0) vectors = 1;
        else if (strcmp(argv[i], "--diff") == 0 && i + 1 < argc) g_difficulty = atoi(argv[++i]);
        else if (strcmp(argv[i], "--blocks") == 0 && i + 1 < argc) max_blocks = atol(argv[++i]);
    }

    if (self_test() != 0) return 1;
    if (vectors) { print_vectors(); return 0; }

    key_load_or_create(&g_keys[0], "key_a.txt");
    key_load_or_create(&g_keys[1], "key_b.txt");
    chain_init();

    static State st;
    static Tx txs[MAX_TX];
    char line[160], hex[65];

    for (long mined = 0; max_blocks < 0 || mined < max_blocks; mined++) {
        state_at(g_tip, &st);
        demo_transactions(&st);
        draw_screen(&st);
        printf("\nRudarim blok %llu...\n", (unsigned long long)g_blocks[g_tip].height + 1);
        fflush(stdout);

        Block b;
        memset(&b, 0, sizeof b);
        b.height = g_blocks[g_tip].height + 1;
        memcpy(b.prev, g_blocks[g_tip].hash, 32);
        b.time = (uint64_t)time(NULL);
        if (b.time < g_blocks[g_tip].time) b.time = g_blocks[g_tip].time;
        memcpy(b.miner, g_keys[0].pk, 32);
        b.ntx = (uint16_t)select_txs(&st, txs);
        b.tx = txs;
        compute_txroot(txs, b.ntx, b.txroot);
        mine(&b);

        const char *why;
        if (add_block(&b, &why) != 0) {
            snprintf(line, sizeof line, "GRESKA: sopstveni blok odbijen: %s", why);
            logf_line(line);
            continue;
        }
        const Block *nb = &g_blocks[g_nblocks - 1];
        to_hex(hex, nb->hash, 32);
        if (nb->reward >= 0)
            snprintf(line, sizeof line, "blok %llu: %.16s...  za %.1f s  %d tx  nagrada parcela %d (%d,%d)",
                     (unsigned long long)nb->height, hex, g_last_secs, nb->ntx, nb->reward,
                     nb->reward % CANVAS_W, nb->reward / CANVAS_W);
        else
            snprintf(line, sizeof line, "blok %llu: %.16s...  za %.1f s  %d tx  bez nagrade (sve zauzeto)",
                     (unsigned long long)nb->height, hex, g_last_secs, nb->ntx);
        logf_line(line);

        state_at(g_tip, &st);
        prune_mempool(&st);
        save_bmp(&st, "canvas.bmp");
        static char json[64 * 1024];
        block_to_json(nb, json, sizeof json);
        FILE *f = fopen("last_block.json", "w");
        if (f) { fprintf(f, "%s\n", json); fclose(f); }
    }
    state_at(g_tip, &st);
    draw_screen(&st);
    return 0;
}