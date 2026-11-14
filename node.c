/*
 * Pixel Chain - mrezni cvor (milestone 3), C + libsodium
 * Prati SPEC.md (verzija iz PROTOTYPE_DEFAULTS).
 *
 * Kompajliranje (Windows):  gcc node.c -o node.exe -O2 -static -lsodium -lws2_32
 * Kompajliranje (Linux):    gcc node.c -o node -O2 -lsodium
 *
 * Najjednostavnije: cvorovi se sami pronalaze preko UDP broadcast-a (port 7000):
 *   node.exe --port 7001 --mine          (na svakom racunaru u istoj mrezi)
 *
 * Primer, dva cvora na istom racunaru:
 *   node.exe --port 7001 --peers 7002 --data cvor1 --mine
 *   node.exe --port 7002 --peers 7001 --data cvor2 --mine
 *
 * Primer, dva racunara:
 *   node.exe --listen 0.0.0.0:7001 --peers 192.168.1.20:7001 --mine
 *   node.exe --listen 0.0.0.0:7001 --peers 192.168.1.10:7001 --mine
 *
 * Interfejs u browseru: http://localhost:<port + 1000>, npr. http://localhost:8001
 *
 * Podesavanja se citaju iz config.txt (pravi se sam pri prvom pokretanju).
 *
 * Opcije:
 *   --port N            port za slusanje (isto sto i --listen 0.0.0.0:N)
 *   --listen ADR:PORT   adresa za slusanje (podrazumevano 0.0.0.0:7001)
 *   --peers A:P,B:P     ostali cvorovi (moze vise puta); samo port = isti racunar (127.0.0.1)
 *   --data FOLDER       folder za kljuceve i slike (podrazumevano data)
 *   --mine              ukljuci rudarenje (podrazumevano iskljuceno)
 *   --no-lag            iskljuci simulaciju laga (podrazumevano ukljucena)
 *   --lag-seed N        seme za lag (podrazumevano 1)
 *   --no-demo           ne pravi automatske transakcije
 *   --config FAJL       drugi config fajl (podrazumevano config.txt)
 *   --no-config         ne citaj config.txt
 *   --no-discover       iskljuci automatsko pronalazenje cvorova (UDP broadcast)
 *   --discover-port N   UDP port za pronalazenje (podrazumevano 7000)
 *   --vectors           ispisi test podatke i izadji
 */
#define _CRT_SECURE_NO_WARNINGS
#ifdef _WIN32
#define FD_SETSIZE 256
#define _WIN32_WINNT 0x0601
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include <direct.h>
#include <conio.h>
#ifndef ENABLE_VIRTUAL_TERMINAL_PROCESSING
#define ENABLE_VIRTUAL_TERMINAL_PROCESSING 0x0004
#endif
typedef SOCKET sock_t;
#define BAD_SOCK INVALID_SOCKET
#define sock_close closesocket
#define MKDIR(p) _mkdir(p)
#else
#define _POSIX_C_SOURCE 200809L
#include <sys/types.h>
#include <sys/socket.h>
#include <sys/select.h>
#include <sys/stat.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <signal.h>
#include <termios.h>
typedef int sock_t;
#define BAD_SOCK (-1)
#define sock_close close
#define MKDIR(p) mkdir(p, 0700)
#endif

#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <time.h>
#include <sodium.h>

/* ======================= Konstante iz specifikacije ======================= */

#define CANVAS_W        64
#define CANVAS_H        32
#define NPARCELS        2048
#define DIFFICULTY      23
#define MAX_TX          256
#define FUTURE_LIMIT    120          /* sekundi u buducnosti */
#define ACT_PAINT       1
#define ACT_TRANSFER    2
#define HEADER_LEN      137
#define NONCE_OFFSET    129
#define MAX_LINE        262144
#define GENESIS_HEX     "56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce"

/* Lokalna podesavanja (nisu konsenzus) */
#define MAX_SESSIONS    64
#define MAX_PEERS       32
#define MAX_QUEUE       128
#define MAX_PENDING     1024
#define MAX_WANTED      1024
#define MAX_MEMPOOL     4096
#define TICK_MS         2000
#define RECONNECT_MS    2000
#define CONNECT_TO_MS   5000
#define IDLE_MS         30000
#define MINE_SLICE      16384

/* ======================= Pomocne funkcije ======================= */

static void put_u16(uint8_t *b, uint16_t v) { b[0] = (uint8_t)(v >> 8); b[1] = (uint8_t)v; }
static void put_u64(uint8_t *b, uint64_t v) { for (int i = 7; i >= 0; i--) { b[i] = (uint8_t)v; v >>= 8; } }

static void to_hex(char *out, const uint8_t *in, size_t len)
{
    static const char d[] = "0123456789abcdef";
    for (size_t i = 0; i < len; i++) { out[2*i] = d[in[i] >> 4]; out[2*i+1] = d[in[i] & 15]; }
    out[2*len] = 0;
}

/* Strogo: tacna duzina, samo mala slova */
static int from_hex(uint8_t *out, size_t len, const char *s, size_t slen)
{
    if (slen != 2 * len) return -1;
    for (size_t i = 0; i < len; i++) {
        int v = 0;
        for (int k = 0; k < 2; k++) {
            char c = s[2*i+k];
            int x;
            if (c >= '0' && c <= '9') x = c - '0';
            else if (c >= 'a' && c <= 'f') x = c - 'a' + 10;
            else return -1;
            v = v * 16 + x;
        }
        out[i] = (uint8_t)v;
    }
    return 0;
}

static int leading_zero_bits(const uint8_t *h)
{
    int n = 0;
    for (int i = 0; i < 32; i++) {
        if (h[i] == 0) { n += 8; continue; }
        for (int bit = 7; bit >= 0; bit--) { if (h[i] & (1u << bit)) return n; n++; }
    }
    return n;
}

static uint64_t now_ms(void)
{
#ifdef _WIN32
    return (uint64_t)GetTickCount64();
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000;
#endif
}

static uint64_t unix_now(void) { return (uint64_t)time(NULL); }

/* Javni kljuc je ispravan i "ne slab": tacka na krivoj, kanonska, glavna podgrupa, nije malog reda */
static int key_ok(const uint8_t pk[32]) { return crypto_core_ed25519_is_valid_point(pk) == 1; }

/* ======================= Log poruke za ekran ======================= */

#define NLOG 10
static char g_log[NLOG][200];
static int  g_echo_log = 0;
static int  g_nlog = 0;
static void logmsg(const char *fmt, ...)
{
    char buf[200];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap);
    if (g_nlog == NLOG) { memmove(g_log[0], g_log[1], sizeof g_log[0] * (NLOG - 1)); g_nlog = NLOG - 1; }
    snprintf(g_log[g_nlog++], sizeof g_log[0], "%s", buf);
    if (g_echo_log) { printf("[log] %s\n", buf); fflush(stdout); }
}

/* ======================= Transakcije ======================= */

typedef struct {
    uint8_t  action;          /* 1 = bojenje, 2 = prenos */
    uint16_t parcel;
    uint64_t counter;         /* mora biti = sledeci brojac parcele */
    uint8_t  sender[32];
    uint8_t  color[3];        /* samo za bojenje */
    uint8_t  recipient[32];   /* samo za prenos */
    uint8_t  sig[64];
} Tx;

/* Poruka koja se potpisuje: "PIXEL-TX-v1\0" | akcija | parcela | brojac | posiljalac | sadrzaj
 * Duzina: 58 (bojenje) ili 87 (prenos). */
static size_t tx_msg(const Tx *t, uint8_t out[87])
{
    memcpy(out, "PIXEL-TX-v1", 11); out[11] = 0;
    out[12] = t->action;
    put_u16(out + 13, t->parcel);
    put_u64(out + 15, t->counter);
    memcpy(out + 23, t->sender, 32);
    if (t->action == ACT_PAINT) { memcpy(out + 55, t->color, 3); return 58; }
    memcpy(out + 55, t->recipient, 32);
    return 87;
}

/* Kanonska potpisana transakcija: poruka + potpis (122 ili 151 bajt) */
static size_t tx_canon(const Tx *t, uint8_t out[151])
{
    size_t n = tx_msg(t, out);
    memcpy(out + n, t->sig, 64);
    return n + 64;
}

/* ID = SHA-256("PIXEL-ID-v1\0" + kanonska transakcija) */
static void tx_id(const Tx *t, uint8_t out[32])
{
    uint8_t buf[12 + 151];
    memcpy(buf, "PIXEL-ID-v1", 11); buf[11] = 0;
    size_t n = tx_canon(t, buf + 12);
    crypto_hash_sha256(out, buf, 12 + n);
}

typedef struct { uint8_t seed[32], pk[32], sk[64]; } Key;

static void key_from_seed(Key *k, const uint8_t seed[32])
{
    memcpy(k->seed, seed, 32);
    crypto_sign_seed_keypair(k->pk, k->sk, seed);
}

static void tx_sign(Tx *t, const Key *k)
{
    uint8_t m[87];
    memcpy(t->sender, k->pk, 32);
    size_t n = tx_msg(t, m);
    crypto_sign_detached(t->sig, NULL, m, n, k->sk);
}

/* ======================= Stanje platna ======================= */

typedef struct {
    uint8_t  owned;
    uint8_t  owner[32];
    uint8_t  color[3];       /* pocetno crno */
    uint64_t counter;        /* sledeci brojac, pocetno 0 */
} Parcel;

typedef struct { Parcel p[NPARCELS]; } State;

/* Provera transakcije redom iz specifikacije, pa primena. 0 = ispravna.
 * verify = 0 preskace proveru potpisa (koristi se samo za vec proverene blokove). */
static int tx_apply(State *st, const Tx *t, int verify)
{
    uint8_t m[87];
    if (t->parcel >= NPARCELS) return 1;                                   /* 1. opseg */
    if (verify) {                                                           /* 2. kljuc i potpis */
        if (!key_ok(t->sender)) return 2;
        size_t n = tx_msg(t, m);
        if (crypto_sign_verify_detached(t->sig, m, n, t->sender) != 0) return 2;
    }
    if (t->action == ACT_TRANSFER && !key_ok(t->recipient)) return 3;       /* 3. primalac */
    Parcel *p = &st->p[t->parcel];
    if (!p->owned || memcmp(p->owner, t->sender, 32) != 0) return 4;       /* 4. vlasnik */
    if (t->counter != p->counter || p->counter == UINT64_MAX) return 5;    /* 5. brojac */

    if (t->action == ACT_PAINT) memcpy(p->color, t->color, 3);
    else                        memcpy(p->owner, t->recipient, 32);         /* boja ostaje */
    p->counter++;
    return 0;
}

static const char *tx_err(int e)
{
    static const char *s[] = { "ok", "parcela van opsega", "los kljuc ili potpis",
                               "los kljuc primaoca", "posiljalac nije vlasnik", "pogresan brojac" };
    return (e >= 0 && e <= 5) ? s[e] : "?";
}

/* Dijagnosticki hes stanja: za svaku parcelu red po red
 * vlasnik postoji (1) | vlasnik (32, nule ako ga nema) | RGB (3) | brojac u64. Nije konsenzus. */
static void state_hash(const State *st, uint8_t out[32])
{
    crypto_hash_sha256_state h;
    crypto_hash_sha256_init(&h);
    for (int i = 0; i < NPARCELS; i++) {
        uint8_t b[44];
        b[0] = st->p[i].owned ? 1 : 0;
        if (st->p[i].owned) memcpy(b + 1, st->p[i].owner, 32); else memset(b + 1, 0, 32);
        memcpy(b + 33, st->p[i].color, 3);
        put_u64(b + 36, st->p[i].counter);
        crypto_hash_sha256_update(&h, b, 44);
    }
    crypto_hash_sha256_final(&h, out);
}

/* ======================= Blokovi ======================= */

typedef struct {
    uint8_t  prev[32];
    uint64_t height;
    uint8_t  miner[32];
    uint64_t time;
    uint16_t ntx;
    uint8_t  commit[32];
    uint64_t nonce;
    Tx      *tx;
    uint8_t  hash[32];
    int      parent;          /* indeks roditelja u lancu */
    int      reward;          /* dobijena parcela, -1 ako nijedna */
} Block;

/* Commitment = SHA-256( za svaku tx: u16 duzina | kanonska tx ) */
static void compute_commit(const Tx *txs, int n, uint8_t out[32])
{
    crypto_hash_sha256_state h;
    uint8_t buf[2 + 151];
    crypto_hash_sha256_init(&h);
    for (int i = 0; i < n; i++) {
        size_t len = tx_canon(&txs[i], buf + 2);
        put_u16(buf, (uint16_t)len);
        crypto_hash_sha256_update(&h, buf, 2 + len);
    }
    crypto_hash_sha256_final(&h, out);
}

/* Zaglavlje (137 bajtova): "PIXEL-BLOCK-v1\0" | prev | height | miner | time | ntx | commit | nonce */
static void block_header(const Block *b, uint8_t out[HEADER_LEN])
{
    memcpy(out, "PIXEL-BLOCK-v1", 14); out[14] = 0;
    memcpy(out + 15, b->prev, 32);
    put_u64(out + 47, b->height);
    memcpy(out + 55, b->miner, 32);
    put_u64(out + 87, b->time);
    put_u16(out + 95, b->ntx);
    memcpy(out + 97, b->commit, 32);
    put_u64(out + NONCE_OFFSET, b->nonce);
}

static void block_hash(const Block *b, uint8_t out[32])
{
    uint8_t h[HEADER_LEN];
    block_header(b, h);
    crypto_hash_sha256(out, h, HEADER_LEN);
}

/* Nagrada: p0 = poslednja 2 bajta hesa mod 2048; ako je zauzeta, p = (p * (time mod 2048)) mod 2048.
 * Najvise 2048 kandidata ukupno; ako nijedan nije slobodan, nema nagrade. */
static int reward_parcel(const State *st, const uint8_t hash[32], uint64_t time_)
{
    uint32_t p = (((uint32_t)hash[30] << 8) | hash[31]) % NPARCELS;
    uint32_t m = (uint32_t)(time_ % NPARCELS);
    for (int i = 0; i < NPARCELS; i++) {
        if (!st->p[p].owned) return (int)p;
        p = (p * m) % NPARCELS;
    }
    return -1;
}

/* Primeni blok: transakcije redom, pa nagrada. Stanje se menja samo ako je sve ispravno. */
static int block_apply(State *st, Block *b, int verify, int *bad_tx)
{
    static State tmp;
    if (b->height == 0) { b->reward = -1; return 0; }
    tmp = *st;
    for (int i = 0; i < b->ntx; i++) {
        int e = tx_apply(&tmp, &b->tx[i], verify);
        if (e) { if (bad_tx) *bad_tx = e; return -1; }
    }
    int p = reward_parcel(&tmp, b->hash, b->time);
    if (p >= 0) { tmp.p[p].owned = 1; memcpy(tmp.p[p].owner, b->miner, 32); }
    b->reward = p;
    *st = tmp;
    return 0;
}

static void make_genesis(Block *g)
{
    memset(g, 0, sizeof *g);
    compute_commit(NULL, 0, g->commit);
    block_hash(g, g->hash);
    g->parent = -1;
    g->reward = -1;
}

/* ======================= Lanac ======================= */

static Block *g_blocks = NULL;
static int    g_nblocks = 0, g_bcap = 0;
static int    g_tip = 0;
static State  g_tip_state;                /* stanje posle vrha */
static uint64_t g_tip_gen = 0;            /* raste sa svakom promenom vrha */

static int find_block(const uint8_t h[32])
{
    for (int i = g_nblocks - 1; i >= 0; i--)
        if (memcmp(g_blocks[i].hash, h, 32) == 0) return i;
    return -1;
}

/* Stanje posle bloka idx: primena od genesis-a (potpisi su vec provereni) */
static void state_at(int idx, State *st)
{
    static int path[1 << 20];
    int n = 0;
    for (int i = idx; i >= 0; i = g_blocks[i].parent) path[n++] = i;
    memset(st, 0, sizeof *st);
    for (int i = n - 1; i >= 0; i--) block_apply(st, &g_blocks[path[i]], 0, NULL);
}

/* Vise rada = veca visina (svaki blok 2^23). Izjednacenje: manji hes. */
static int better(int a, int b)
{
    if (g_blocks[a].height != g_blocks[b].height) return g_blocks[a].height > g_blocks[b].height;
    return memcmp(g_blocks[a].hash, g_blocks[b].hash, 32) < 0;
}

static void chain_init(void)
{
    g_bcap = 256;
    g_blocks = malloc(sizeof(Block) * g_bcap);
    make_genesis(&g_blocks[0]);
    g_nblocks = 1;
    g_tip = 0;
    memset(&g_tip_state, 0, sizeof g_tip_state);
}

/* ======================= Mempool ======================= */

static Tx  g_mem[MAX_MEMPOOL];
static int g_nmem = 0;
static State g_mem_state;                 /* stanje vrha + sve iz mempool-a, redom */
static uint64_t g_mem_gen = 0;

static void mempool_rebuild(const Tx *extra, int nextra)
{
    static Tx old[MAX_MEMPOOL * 2];
    int n = 0;
    for (int i = 0; i < nextra && n < MAX_MEMPOOL * 2; i++) old[n++] = extra[i];
    for (int i = 0; i < g_nmem && n < MAX_MEMPOOL * 2; i++) old[n++] = g_mem[i];
    g_mem_state = g_tip_state;
    g_nmem = 0;
    for (int i = 0; i < n && g_nmem < MAX_MEMPOOL; i++) {
        uint8_t id[32], id2[32];
        int dup = 0;
        tx_id(&old[i], id);
        for (int j = 0; j < g_nmem; j++) { tx_id(&g_mem[j], id2); if (!memcmp(id, id2, 32)) { dup = 1; break; } }
        if (dup) continue;
        if (tx_apply(&g_mem_state, &old[i], 0) == 0) g_mem[g_nmem++] = old[i];
    }
    g_mem_gen++;
}

/* Prihvati transakciju ako je ispravna nad stanjem vrha + mempool. 0 = nova i prihvacena. */
static int mempool_add(const Tx *t, int *err)
{
    uint8_t id[32], id2[32];
    tx_id(t, id);
    for (int i = 0; i < g_nmem; i++) { tx_id(&g_mem[i], id2); if (!memcmp(id, id2, 32)) { *err = -1; return -1; } }
    if (g_nmem >= MAX_MEMPOOL) { *err = -2; return -1; }
    int e = tx_apply(&g_mem_state, t, 1);
    if (e) { *err = e; return -1; }
    g_mem[g_nmem++] = *t;
    g_mem_gen++;
    return 0;
}

/* Promena vrha: novo stanje, transakcije sa napustene grane nazad u mempool */
static void set_tip(int new_tip)
{
    static Tx detached[MAX_MEMPOOL];
    int nd = 0;
    int a = g_tip, b = new_tip;
    /* nadji zajednickog pretka; skupi transakcije sa stare grane */
    static int old_path[1 << 16];
    int nold = 0;
    while (a != b) {
        if (g_blocks[a].height >= g_blocks[b].height) {
            if (nold < (1 << 16)) old_path[nold++] = a;
            a = g_blocks[a].parent;
        } else b = g_blocks[b].parent;
    }
    for (int i = nold - 1; i >= 0; i--) {
        Block *ob = &g_blocks[old_path[i]];
        for (int k = 0; k < ob->ntx && nd < MAX_MEMPOOL; k++) detached[nd++] = ob->tx[k];
    }
    if (nold > 0 && g_blocks[new_tip].parent != g_tip)
        logmsg("reorganizacija: napustena grana od %d blokova", nold);
    g_tip = new_tip;
    state_at(g_tip, &g_tip_state);
    g_tip_gen++;
    mempool_rebuild(detached, nd);
}

/* ======================= Blokovi na cekanju i trazeni hesevi ======================= */

typedef struct { Block b; int used; uint64_t added; } Pending;
static Pending g_pending[MAX_PENDING];
static int     g_npending = 0;

static uint8_t g_wanted[MAX_WANTED][32];
static int     g_nwanted = 0, g_want_cursor = 0;

static int pending_find(const uint8_t h[32])
{
    for (int i = 0; i < MAX_PENDING; i++)
        if (g_pending[i].used && !memcmp(g_pending[i].b.hash, h, 32)) return i;
    return -1;
}

static int is_known(const uint8_t h[32]) { return find_block(h) >= 0 || pending_find(h) >= 0; }

static void wanted_remove(const uint8_t h[32])
{
    for (int i = 0; i < g_nwanted; i++)
        if (!memcmp(g_wanted[i], h, 32)) { memmove(g_wanted[i], g_wanted[i+1], 32 * (g_nwanted - i - 1)); g_nwanted--; return; }
}

static void wanted_add(const uint8_t h[32])
{
    if (is_known(h)) return;
    for (int i = 0; i < g_nwanted; i++) if (!memcmp(g_wanted[i], h, 32)) return;
    if (g_nwanted == MAX_WANTED) { memmove(g_wanted[0], g_wanted[1], 32 * (MAX_WANTED - 1)); g_nwanted--; }
    memcpy(g_wanted[g_nwanted++], h, 32);
}

static void free_block(Block *b) { free(b->tx); b->tx = NULL; }

static void pending_store(const Block *b)
{
    int slot = -1;
    for (int i = 0; i < MAX_PENDING; i++) if (!g_pending[i].used) { slot = i; break; }
    if (slot < 0) {            /* puno: izbaci najvisi blok (preci imaju prednost) */
        uint64_t hi = 0;
        for (int i = 0; i < MAX_PENDING; i++) if (g_pending[i].b.height >= hi) { hi = g_pending[i].b.height; slot = i; }
        if (b->height >= hi) return;
        free_block(&g_pending[slot].b);
        g_npending--;
    }
    g_pending[slot].b = *b;
    g_pending[slot].b.tx = NULL;
    if (b->ntx) {
        g_pending[slot].b.tx = malloc(sizeof(Tx) * b->ntx);
        memcpy(g_pending[slot].b.tx, b->tx, sizeof(Tx) * b->ntx);
    }
    g_pending[slot].used = 1;
    g_pending[slot].added = now_ms();
    g_npending++;
    wanted_remove(b->hash);
}

/* ======================= Prijem bloka ======================= */

enum { B_ADDED, B_KNOWN, B_PENDING, B_FUTURE, B_INVALID };
static const char *g_bad_reason = "";

/* Provera redom iz specifikacije. Blok se kopira (transakcije takodje). */
static int block_accept(const Block *in)
{
    if (find_block(in->hash) >= 0) return B_KNOWN;
    if (in->ntx > MAX_TX) { g_bad_reason = "previse transakcija"; return B_INVALID; }
    int parent = find_block(in->prev);
    if (parent < 0) return B_PENDING;
    const Block *pb = &g_blocks[parent];
    if (pb->height == UINT64_MAX || in->height != pb->height + 1) { g_bad_reason = "pogresna visina"; return B_INVALID; }
    if (!key_ok(in->miner)) { g_bad_reason = "los kljuc rudara"; return B_INVALID; }
    if (in->time < pb->time) { g_bad_reason = "vreme pre roditelja"; return B_INVALID; }
    if (in->time > unix_now() + FUTURE_LIMIT) return B_FUTURE;
    if (leading_zero_bits(in->hash) < DIFFICULTY) { g_bad_reason = "nedovoljno rada"; return B_INVALID; }

    static State st;
    if (parent == g_tip) st = g_tip_state; else state_at(parent, &st);
    Block b = *in;
    b.tx = NULL;
    if (b.ntx) { b.tx = malloc(sizeof(Tx) * b.ntx); memcpy(b.tx, in->tx, sizeof(Tx) * b.ntx); }
    b.parent = parent;
    int bad = 0;
    if (block_apply(&st, &b, 1, &bad) != 0) {
        free(b.tx);
        g_bad_reason = tx_err(bad);
        return B_INVALID;
    }
    if (g_nblocks == g_bcap) { g_bcap *= 2; g_blocks = realloc(g_blocks, sizeof(Block) * g_bcap); }
    g_blocks[g_nblocks++] = b;
    wanted_remove(b.hash);
    if (better(g_nblocks - 1, g_tip)) set_tip(g_nblocks - 1);
    return B_ADDED;
}

/* ======================= JSON ======================= */
/* Mali strogi parser: objekti, nizovi, stringovi, nenegativni celi brojevi (u64), true/false/null. */

enum { J_NULL, J_BOOL, J_NUM, J_STR, J_ARR, J_OBJ };
typedef struct {
    int t;
    uint64_t num;
    const char *s; int slen;      /* string vrednost */
    const char *k; int klen;      /* kljuc (ako je clan objekta) */
    int child, next, count;
} JNode;

#define JPOOL 32768
static JNode g_j[JPOOL];
static int   g_jn;
static char  g_jstr[MAX_LINE + 16];
static int   g_jsn;
static const char *g_jp, *g_jend;

static void jws(void) { while (g_jp < g_jend && (*g_jp == ' ' || *g_jp == '\t' || *g_jp == '\r' || *g_jp == '\n')) g_jp++; }

static int jnew(int t)
{
    if (g_jn >= JPOOL) return -1;
    memset(&g_j[g_jn], 0, sizeof(JNode));
    g_j[g_jn].t = t; g_j[g_jn].child = g_j[g_jn].next = -1;
    return g_jn++;
}

static int jstring(const char **out, int *outlen)
{
    if (g_jp >= g_jend || *g_jp != '"') return -1;
    g_jp++;
    char *dst = g_jstr + g_jsn;
    int n = 0;
    while (g_jp < g_jend && *g_jp != '"') {
        unsigned char c = (unsigned char)*g_jp++;
        if (c < 0x20) return -1;
        if (c == '\\') {
            if (g_jp >= g_jend) return -1;
            char e = *g_jp++;
            switch (e) {
            case '"': c = '"'; break;  case '\\': c = '\\'; break; case '/': c = '/'; break;
            case 'b': c = '\b'; break; case 'f': c = '\f'; break;  case 'n': c = '\n'; break;
            case 'r': c = '\r'; break; case 't': c = '\t'; break;
            case 'u': {
                if (g_jend - g_jp < 4) return -1;
                unsigned v = 0;
                for (int i = 0; i < 4; i++) {
                    char h = *g_jp++; v <<= 4;
                    if (h >= '0' && h <= '9') v |= h - '0';
                    else if (h >= 'a' && h <= 'f') v |= h - 'a' + 10;
                    else if (h >= 'A' && h <= 'F') v |= h - 'A' + 10;
                    else return -1;
                }
                if (v >= 0xD800 && v <= 0xDFFF) return -1;
                if (v < 0x80) { dst[n++] = (char)v; }
                else if (v < 0x800) { dst[n++] = (char)(0xC0 | (v >> 6)); dst[n++] = (char)(0x80 | (v & 63)); }
                else { dst[n++] = (char)(0xE0 | (v >> 12)); dst[n++] = (char)(0x80 | ((v >> 6) & 63)); dst[n++] = (char)(0x80 | (v & 63)); }
                continue;
            }
            default: return -1;
            }
        }
        dst[n++] = (char)c;
    }
    if (g_jp >= g_jend) return -1;
    g_jp++;
    *out = dst; *outlen = n;
    g_jsn += n;
    return 0;
}

static int jvalue(int depth)
{
    if (depth > 32) return -1;
    jws();
    if (g_jp >= g_jend) return -1;
    char c = *g_jp;
    if (c == '{' || c == '[') {
        int obj = (c == '{');
        int me = jnew(obj ? J_OBJ : J_ARR);
        if (me < 0) return -1;
        g_jp++;
        jws();
        int last = -1;
        if (g_jp < g_jend && *g_jp == (obj ? '}' : ']')) { g_jp++; return me; }
        for (;;) {
            const char *k = NULL; int kl = 0;
            jws();
            if (obj) {
                if (jstring(&k, &kl)) return -1;
                jws();
                if (g_jp >= g_jend || *g_jp != ':') return -1;
                g_jp++;
            }
            int v = jvalue(depth + 1);
            if (v < 0) return -1;
            g_j[v].k = k; g_j[v].klen = kl;
            if (last < 0) g_j[me].child = v; else g_j[last].next = v;
            last = v;
            g_j[me].count++;
            jws();
            if (g_jp >= g_jend) return -1;
            if (*g_jp == ',') { g_jp++; continue; }
            if (*g_jp == (obj ? '}' : ']')) { g_jp++; return me; }
            return -1;
        }
    }
    if (c == '"') {
        int me = jnew(J_STR);
        if (me < 0 || jstring(&g_j[me].s, &g_j[me].slen)) return -1;
        return me;
    }
    if (c >= '0' && c <= '9') {
        int me = jnew(J_NUM);
        if (me < 0) return -1;
        if (c == '0' && g_jp + 1 < g_jend && g_jp[1] >= '0' && g_jp[1] <= '9') return -1;   /* vodece nule */
        uint64_t v = 0;
        while (g_jp < g_jend && *g_jp >= '0' && *g_jp <= '9') {
            uint64_t d = (uint64_t)(*g_jp - '0');
            if (v > (UINT64_MAX - d) / 10) return -1;                                       /* prekoracenje */
            v = v * 10 + d;
            g_jp++;
        }
        if (g_jp < g_jend && (*g_jp == '.' || *g_jp == 'e' || *g_jp == 'E')) return -1;    /* samo celi brojevi */
        g_j[me].num = v;
        return me;
    }
    if (g_jend - g_jp >= 4 && !memcmp(g_jp, "true", 4))  { g_jp += 4; int me = jnew(J_BOOL); if (me >= 0) g_j[me].num = 1; return me; }
    if (g_jend - g_jp >= 5 && !memcmp(g_jp, "false", 5)) { g_jp += 5; return jnew(J_BOOL); }
    if (g_jend - g_jp >= 4 && !memcmp(g_jp, "null", 4))  { g_jp += 4; return jnew(J_NULL); }
    return -1;
}

static int utf8_ok(const unsigned char *s, size_t n)
{
    size_t i = 0;
    while (i < n) {
        unsigned char c = s[i];
        int len;
        uint32_t cp;
        if (c < 0x80) { i++; continue; }
        else if ((c & 0xE0) == 0xC0) { len = 2; cp = c & 0x1F; }
        else if ((c & 0xF0) == 0xE0) { len = 3; cp = c & 0x0F; }
        else if ((c & 0xF8) == 0xF0) { len = 4; cp = c & 0x07; }
        else return 0;
        if (i + len > n) return 0;
        for (int k = 1; k < len; k++) { if ((s[i+k] & 0xC0) != 0x80) return 0; cp = (cp << 6) | (s[i+k] & 0x3F); }
        if ((len == 2 && cp < 0x80) || (len == 3 && cp < 0x800) || (len == 4 && (cp < 0x10000 || cp > 0x10FFFF))) return 0;
        if (cp >= 0xD800 && cp <= 0xDFFF) return 0;
        i += len;
    }
    return 1;
}

/* Parsiraj ceo red. Vraca indeks korena ili -1. */
static int json_parse(const char *s, size_t n)
{
    if (!utf8_ok((const unsigned char *)s, n)) return -1;
    g_jn = 0; g_jsn = 0; g_jp = s; g_jend = s + n;
    int r = jvalue(0);
    if (r < 0) return -1;
    jws();
    if (g_jp != g_jend) return -1;
    return r;
}

static int jget(int obj, const char *key)
{
    if (obj < 0 || g_j[obj].t != J_OBJ) return -1;
    int kl = (int)strlen(key);
    for (int c = g_j[obj].child; c >= 0; c = g_j[c].next)
        if (g_j[c].klen == kl && !memcmp(g_j[c].k, key, kl)) return c;
    return -1;
}

static int jstr_is(int n, const char *v)
{
    return n >= 0 && g_j[n].t == J_STR && g_j[n].slen == (int)strlen(v) && !memcmp(g_j[n].s, v, g_j[n].slen);
}

static int jhex(int n, uint8_t *out, size_t len)
{
    return (n >= 0 && g_j[n].t == J_STR) ? from_hex(out, len, g_j[n].s, g_j[n].slen) : -1;
}

static int jnum(int n, uint64_t *out)
{
    if (n < 0 || g_j[n].t != J_NUM) return -1;
    *out = g_j[n].num;
    return 0;
}

/* Transakcija iz JSON-a. -1 = losa polja (zatvara sesiju). */
static int json_to_tx(int o, Tx *t)
{
    uint64_t v;
    memset(t, 0, sizeof *t);
    if (o < 0 || g_j[o].t != J_OBJ || g_j[o].count != 5) return -1;
    if (jnum(jget(o, "parcel"), &v) || v > 0xFFFF) return -1;
    t->parcel = (uint16_t)v;
    if (jnum(jget(o, "counter"), &t->counter)) return -1;
    if (jhex(jget(o, "sender"), t->sender, 32)) return -1;
    if (jhex(jget(o, "signature"), t->sig, 64)) return -1;
    int a = jget(o, "action");
    if (a < 0 || g_j[a].t != J_OBJ || g_j[a].count != 2) return -1;
    int ty = jget(a, "type");
    if (jstr_is(ty, "paint")) {
        t->action = ACT_PAINT;
        if (jhex(jget(a, "color"), t->color, 3)) return -1;
    } else if (jstr_is(ty, "transfer")) {
        t->action = ACT_TRANSFER;
        if (jhex(jget(a, "recipient"), t->recipient, 32)) return -1;
    } else return -1;
    return 0;
}

/* Blok iz JSON-a (bez polja hash; hes racuna primalac). txbuf mora imati mesta za sve transakcije. */
static int json_to_block(int o, Block *b, Tx *txbuf, int txcap)
{
    uint64_t v;
    memset(b, 0, sizeof *b);
    if (o < 0 || g_j[o].t != J_OBJ || g_j[o].count != 6) return -1;
    if (jhex(jget(o, "previous"), b->prev, 32)) return -1;
    if (jnum(jget(o, "height"), &b->height)) return -1;
    if (jhex(jget(o, "miner"), b->miner, 32)) return -1;
    if (jnum(jget(o, "timestamp"), &b->time)) return -1;
    if (jnum(jget(o, "nonce"), &b->nonce)) return -1;
    int arr = jget(o, "transactions");
    if (arr < 0 || g_j[arr].t != J_ARR) return -1;
    if (g_j[arr].count > 0xFFFF || g_j[arr].count > txcap) return -1;
    int i = 0;
    for (int c = g_j[arr].child; c >= 0; c = g_j[c].next, i++)
        if (json_to_tx(c, &txbuf[i])) return -1;
    b->ntx = (uint16_t)i;
    b->tx = txbuf;
    compute_commit(txbuf, i, b->commit);          /* primalac sam racuna commitment */
    (void)v;
    return 0;
}

static int tx_to_json(const Tx *t, char *out, size_t cap)
{
    char sender[65], sig[129], val[65];
    to_hex(sender, t->sender, 32);
    to_hex(sig, t->sig, 64);
    if (t->action == ACT_PAINT) {
        to_hex(val, t->color, 3);
        return snprintf(out, cap, "{\"parcel\":%u,\"counter\":%llu,\"sender\":\"%s\",\"action\":{\"type\":\"paint\",\"color\":\"%s\"},\"signature\":\"%s\"}",
                        t->parcel, (unsigned long long)t->counter, sender, val, sig);
    }
    to_hex(val, t->recipient, 32);
    return snprintf(out, cap, "{\"parcel\":%u,\"counter\":%llu,\"sender\":\"%s\",\"action\":{\"type\":\"transfer\",\"recipient\":\"%s\"},\"signature\":\"%s\"}",
                    t->parcel, (unsigned long long)t->counter, sender, val, sig);
}

static int block_to_json(const Block *b, char *out, size_t cap)
{
    char prev[65], miner[65];
    to_hex(prev, b->prev, 32);
    to_hex(miner, b->miner, 32);
    int n = snprintf(out, cap, "{\"previous\":\"%s\",\"height\":%llu,\"miner\":\"%s\",\"timestamp\":%llu,\"nonce\":%llu,\"transactions\":[",
                     prev, (unsigned long long)b->height, miner, (unsigned long long)b->time, (unsigned long long)b->nonce);
    for (int i = 0; i < b->ntx && n < (int)cap; i++) {
        if (i) n += snprintf(out + n, cap - n, ",");
        n += tx_to_json(&b->tx[i], out + n, cap - n);
    }
    if (n < (int)cap) n += snprintf(out + n, cap - n, "]}");
    return n;
}

/* ======================= Mreza ======================= */

typedef struct { char *data; size_t len; uint64_t at; } QMsg;

typedef struct {
    int      used;
    sock_t   s;
    int      outbound;           /* 1 = mi smo se povezali */
    int      peer;               /* indeks u g_peers za odlazne */
    int      connecting;
    uint64_t connect_start;
    int      ready;              /* primljen ispravan hello */
    uint8_t  identity[32];
    char    *rbuf; size_t rlen;
    char    *wbuf; size_t wlen, wcap;
    QMsg     q[MAX_QUEUE]; int qhead, qn;
    uint64_t last_deliver;
    uint64_t last_rx;
    uint64_t rng;                /* xorshift64 za lag */
    char     name[64];
} Session;

typedef struct { char host[64]; int port; struct sockaddr_in addr; uint64_t next_try; int session; int has_id; uint8_t id[32]; } Peer;

static Session g_s[MAX_SESSIONS];
static Peer    g_peers[MAX_PEERS];
static int     g_npeers = 0;
static sock_t  g_listen = BAD_SOCK;
static sock_t  g_udp = BAD_SOCK;            /* otkrivanje cvorova (UDP broadcast) */
static int     g_discover = 1, g_discover_port = 7000;
static int     g_lag = 1;
static uint64_t g_lag_seed = 1, g_conn_seq = 0;
static Key     g_id;                         /* identitet cvora = kljuc rudara */
static uint8_t g_genesis[32];

static uint64_t xorshift(uint64_t *x)
{
    uint64_t v = *x;
    v ^= v << 13; v ^= v >> 7; v ^= v << 17;
    return *x = v;
}

static void set_nonblock(sock_t s)
{
#ifdef _WIN32
    u_long one = 1;
    ioctlsocket(s, FIONBIO, &one);
#else
    fcntl(s, F_SETFL, fcntl(s, F_GETFL, 0) | O_NONBLOCK);
#endif
    int one2 = 1;
    setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (const char *)&one2, sizeof one2);
}

static int would_block(void)
{
#ifdef _WIN32
    int e = WSAGetLastError();
    return e == WSAEWOULDBLOCK || e == WSAEINPROGRESS;
#else
    return errno == EWOULDBLOCK || errno == EAGAIN || errno == EINPROGRESS;
#endif
}

static int session_new(sock_t s, int outbound, int peer, const char *name)
{
    for (int i = 0; i < MAX_SESSIONS; i++) {
        if (g_s[i].used) continue;
        Session *x = &g_s[i];
        memset(x, 0, sizeof *x);
        x->used = 1; x->s = s; x->outbound = outbound; x->peer = peer;
        x->rbuf = malloc(MAX_LINE + 4096);
        x->wcap = 65536; x->wbuf = malloc(x->wcap);
        x->last_rx = now_ms();
        x->rng = (g_lag_seed ^ (0x9E3779B97F4A7C15ULL * ++g_conn_seq)) | 1;
        snprintf(x->name, sizeof x->name, "%s", name);
        return i;
    }
    return -1;
}

static void session_close(int i, const char *why)
{
    Session *x = &g_s[i];
    if (!x->used) return;
    if (why && x->ready) logmsg("veza %s zatvorena: %s", x->name, why);
    sock_close(x->s);
    free(x->rbuf); free(x->wbuf);
    for (int k = 0; k < x->qn; k++) free(x->q[(x->qhead + k) % MAX_QUEUE].data);
    if (x->outbound && x->peer >= 0) { g_peers[x->peer].session = -1; g_peers[x->peer].next_try = now_ms() + RECONNECT_MS; }
    x->used = 0;
}

/* Posalji poruku (jedan JSON objekat). Lag: 10% se izgubi, ostale kasne 20-250 ms, redosled ostaje. */
static void send_msg(int i, const char *json)
{
    Session *x = &g_s[i];
    if (!x->used || x->connecting) return;
    uint64_t now = now_ms(), at = now;
    if (g_lag) {
        if (xorshift(&x->rng) % 100 < 10) return;                 /* izgubljena */
        at = now + 20 + xorshift(&x->rng) % 231;                   /* 20..250 ms */
    }
    if (at < x->last_deliver) at = x->last_deliver;               /* FIFO unutar sesije */
    if (x->qn == MAX_QUEUE) { session_close(i, "red za slanje je pun"); return; }
    size_t n = strlen(json);
    QMsg *m = &x->q[(x->qhead + x->qn) % MAX_QUEUE];
    m->data = malloc(n + 1);
    memcpy(m->data, json, n);
    m->data[n] = '\n';
    m->len = n + 1;
    m->at = at;
    x->qn++;
    x->last_deliver = at;
}

static void broadcast(const char *json)
{
    for (int i = 0; i < MAX_SESSIONS; i++) if (g_s[i].used && g_s[i].ready) send_msg(i, json);
}

static void send_hello(int i)
{
    char gh[65], idh[65], buf[256];
    to_hex(gh, g_genesis, 32);
    to_hex(idh, g_id.pk, 32);
    snprintf(buf, sizeof buf, "{\"type\":\"hello\",\"version\":1,\"genesis\":\"%s\",\"identity\":\"%s\"}", gh, idh);
    send_msg(i, buf);
}

static void send_tip(int i)
{
    char h[65], buf[160];
    to_hex(h, g_blocks[g_tip].hash, 32);
    snprintf(buf, sizeof buf, "{\"type\":\"tip\",\"hash\":\"%s\",\"height\":%llu}", h, (unsigned long long)g_blocks[g_tip].height);
    send_msg(i, buf);
}

static void send_get_block(int i, const uint8_t h[32])
{
    char hx[65], buf[128];
    to_hex(hx, h, 32);
    snprintf(buf, sizeof buf, "{\"type\":\"get_block\",\"hash\":\"%s\"}", hx);
    send_msg(i, buf);
}

static char g_big[MAX_LINE + 1024];

static void make_block_msg(const Block *b)
{
    int n = snprintf(g_big, sizeof g_big, "{\"type\":\"block\",\"block\":");
    n += block_to_json(b, g_big + n, sizeof g_big - n);
    snprintf(g_big + n, sizeof g_big - n, "}");
}

static void make_tx_msg(const Tx *t, char *out, size_t cap)
{
    int n = snprintf(out, cap, "{\"type\":\"transaction\",\"transaction\":");
    n += tx_to_json(t, out + n, cap - n);
    snprintf(out + n, cap - n, "}");
}

/* ======================= Obrada primljenih blokova ======================= */

static void after_new_blocks(void);

/* Blok sa mreze ili iz rudarenja. src = sesija izvora (-1 = mi). */
static void handle_block(Block *b, int src)
{
    block_hash(b, b->hash);
    if (is_known(b->hash)) return;
    char hx[65];
    to_hex(hx, b->hash, 32);
    int r = block_accept(b);
    switch (r) {
    case B_ADDED: {
        const Block *nb = &g_blocks[g_nblocks - 1];
        if (src >= 0)
            logmsg("primljen blok %llu %.12s... (%d tx) od %s", (unsigned long long)nb->height, hx, nb->ntx, g_s[src].name);
        make_block_msg(nb);
        broadcast(g_big);                         /* prosledi jednom svima, ukljucujuci izvor */
        after_new_blocks();
        break;
    }
    case B_PENDING:
        if (leading_zero_bits(b->hash) < DIFFICULTY) return;   /* smece ne cuvamo */
        pending_store(b);
        wanted_add(b->prev);
        if (src >= 0) send_get_block(src, b->prev);           /* odmah trazi roditelja od izvora */
        break;
    case B_FUTURE:
        pending_store(b);
        logmsg("blok %.12s... je iz buducnosti, ceka", hx);
        break;
    case B_INVALID:
        logmsg("ODBIJEN blok %.12s...: %s", hx, g_bad_reason);
        break;
    default: break;
    }
}

/* Posle dodavanja bloka: probaj blokove na cekanju ciji je roditelj sada poznat */
static void after_new_blocks(void)
{
    int progress = 1;
    while (progress) {
        progress = 0;
        for (int i = 0; i < MAX_PENDING; i++) {
            if (!g_pending[i].used) continue;
            Block *pb = &g_pending[i].b;
            if (find_block(pb->prev) < 0) continue;
            if (pb->time > unix_now() + FUTURE_LIMIT) continue;
            Block copy = *pb;
            g_pending[i].used = 0;
            g_npending--;
            int r = block_accept(&copy);
            char hx[65]; to_hex(hx, copy.hash, 32);
            if (r == B_ADDED) {
                make_block_msg(&g_blocks[g_nblocks - 1]);
                broadcast(g_big);
                progress = 1;
            } else if (r == B_INVALID) logmsg("ODBIJEN blok %.12s...: %s", hx, g_bad_reason);
            free(copy.tx);
        }
    }
}

static void handle_tx(const Tx *t, int src)
{
    int err = 0;
    if (mempool_add(t, &err) == 0) {
        char buf[1024];
        make_tx_msg(t, buf, sizeof buf);
        broadcast(buf);
        if (src >= 0) logmsg("primljena tx: %s parcele %u od %s", t->action == ACT_PAINT ? "bojenje" : "prenos", t->parcel, g_s[src].name);
    }
}

/* ======================= Obrada jedne poruke ======================= */

static Tx g_rx_tx[4096];

/* Vraca -1 ako sesiju treba zatvoriti */
static int handle_line(int i, const char *line, size_t n)
{
    Session *x = &g_s[i];
    int r = json_parse(line, n);
    if (r < 0 || g_j[r].t != J_OBJ) return -1;
    int ty = jget(r, "type");
    if (ty < 0 || g_j[ty].t != J_STR) return -1;
    int cnt = g_j[r].count;

    if (jstr_is(ty, "hello")) {
        uint64_t ver; uint8_t gen[32], id[32];
        if (cnt != 4 || jnum(jget(r, "version"), &ver) || jhex(jget(r, "genesis"), gen, 32) || jhex(jget(r, "identity"), id, 32)) return -1;
        if (ver != 1 || memcmp(gen, g_genesis, 32) != 0 || !key_ok(id) || !memcmp(id, g_id.pk, 32)) return -1;
        if (!x->ready) {
            x->ready = 1;
            memcpy(x->identity, id, 32);
            char idh[65]; to_hex(idh, id, 32);
            logmsg("povezan sa %s (identitet %.12s...)", x->name, idh);
            send_hello(i);
            send_tip(i);
        }
        return 0;
    }
    if (jstr_is(ty, "get_tip")) {
        if (cnt != 1) return -1;
        if (x->ready) send_tip(i);
        return 0;
    }
    if (jstr_is(ty, "tip")) {
        uint8_t h[32]; uint64_t height;
        if (cnt != 3 || jhex(jget(r, "hash"), h, 32) || jnum(jget(r, "height"), &height)) return -1;
        if (x->ready && !is_known(h)) { wanted_add(h); send_get_block(i, h); }
        return 0;
    }
    if (jstr_is(ty, "get_block")) {
        uint8_t h[32];
        if (cnt != 2 || jhex(jget(r, "hash"), h, 32)) return -1;
        if (!x->ready) return 0;
        int idx = find_block(h);
        if (idx >= 0) { make_block_msg(&g_blocks[idx]); send_msg(i, g_big); }
        return 0;
    }
    if (jstr_is(ty, "block")) {
        Block b;
        if (cnt != 2 || json_to_block(jget(r, "block"), &b, g_rx_tx, 4096)) return -1;
        if (x->ready) handle_block(&b, i);
        return 0;
    }
    if (jstr_is(ty, "transaction")) {
        Tx t;
        if (cnt != 2 || json_to_tx(jget(r, "transaction"), &t)) return -1;
        if (x->ready) handle_tx(&t, i);
        return 0;
    }
    return -1;                                                     /* nepoznat tip */
}

/* ======================= Ulaz/izlaz na socketima ======================= */

static void session_read(int i)
{
    Session *x = &g_s[i];
    char tmp[65536];
    int n = (int)recv(x->s, tmp, sizeof tmp, 0);
    if (n == 0) { session_close(i, "druga strana je zatvorila vezu"); return; }
    if (n < 0) { if (!would_block()) session_close(i, "greska pri citanju"); return; }
    x->last_rx = now_ms();
    for (int k = 0; k < n; k++) {
        char c = tmp[k];
        if (c == '\n') {
            if (handle_line(i, x->rbuf, x->rlen) < 0) { session_close(i, "neispravna poruka"); return; }
            if (!g_s[i].used) return;
            x->rlen = 0;
        } else {
            if (x->rlen >= MAX_LINE) { session_close(i, "predugacak red"); return; }
            x->rbuf[x->rlen++] = c;
        }
    }
}

static void session_flush(int i)
{
    Session *x = &g_s[i];
    uint64_t now = now_ms();
    while (x->qn > 0 && x->q[x->qhead].at <= now) {
        QMsg *m = &x->q[x->qhead];
        if (x->wlen + m->len > x->wcap) {
            if (x->wlen + m->len > 8u * 1024 * 1024) { session_close(i, "izlazni bafer pun"); return; }
            while (x->wlen + m->len > x->wcap) x->wcap *= 2;
            x->wbuf = realloc(x->wbuf, x->wcap);
        }
        memcpy(x->wbuf + x->wlen, m->data, m->len);
        x->wlen += m->len;
        free(m->data);
        x->qhead = (x->qhead + 1) % MAX_QUEUE;
        x->qn--;
    }
    if (x->wlen == 0) return;
#ifdef MSG_NOSIGNAL
    int n = (int)send(x->s, x->wbuf, (int)x->wlen, MSG_NOSIGNAL);
#else
    int n = (int)send(x->s, x->wbuf, (int)x->wlen, 0);
#endif
    if (n < 0) { if (!would_block()) session_close(i, "greska pri slanju"); return; }
    memmove(x->wbuf, x->wbuf + n, x->wlen - n);
    x->wlen -= n;
}

static int parse_addr(const char *s, struct sockaddr_in *a, char *host, int *port)
{
    char h[64];
    const char *c = strrchr(s, ':');
    if (!c || c - s >= (int)sizeof h) return -1;
    memcpy(h, s, c - s); h[c - s] = 0;
    int p = atoi(c + 1);
    if (p <= 0 || p > 65535) return -1;
    memset(a, 0, sizeof *a);
    a->sin_family = AF_INET;
    a->sin_port = htons((uint16_t)p);
    if (inet_pton(AF_INET, h, &a->sin_addr) != 1) return -1;
    if (host) snprintf(host, 64, "%s", h);
    if (port) *port = p;
    return 0;
}

static void start_connect(int pi)
{
    Peer *p = &g_peers[pi];
    sock_t s = socket(AF_INET, SOCK_STREAM, 0);
    if (s == BAD_SOCK) return;
    set_nonblock(s);
    int r = connect(s, (struct sockaddr *)&p->addr, sizeof p->addr);
    if (r != 0 && !would_block()) { sock_close(s); p->next_try = now_ms() + RECONNECT_MS; return; }
    char name[64];
    snprintf(name, sizeof name, "%.40s:%d", p->host, p->port);
    int i = session_new(s, 1, pi, name);
    if (i < 0) { sock_close(s); return; }
    g_s[i].connecting = 1;
    g_s[i].connect_start = now_ms();
    p->session = i;
}

static void connect_done(int i)
{
    Session *x = &g_s[i];
    int err = 0;
    socklen_t len = sizeof err;
    getsockopt(x->s, SOL_SOCKET, SO_ERROR, (char *)&err, &len);
    if (err != 0) { session_close(i, NULL); return; }
    x->connecting = 0;
    x->last_rx = now_ms();
    send_hello(i);
}

static int listen_on(const char *addr)
{
    struct sockaddr_in a;
    if (parse_addr(addr, &a, NULL, NULL)) return -1;
    g_listen = socket(AF_INET, SOCK_STREAM, 0);
    if (g_listen == BAD_SOCK) return -1;
    int one = 1;
    setsockopt(g_listen, SOL_SOCKET, SO_REUSEADDR, (const char *)&one, sizeof one);
    if (bind(g_listen, (struct sockaddr *)&a, sizeof a) != 0) return -1;
    if (listen(g_listen, 16) != 0) return -1;
    set_nonblock(g_listen);
    return 0;
}

static void accept_new(void)
{
    struct sockaddr_in a;
    socklen_t len = sizeof a;
    sock_t s = accept(g_listen, (struct sockaddr *)&a, &len);
    if (s == BAD_SOCK) return;
    set_nonblock(s);
    char name[64], ip[32];
    inet_ntop(AF_INET, &a.sin_addr, ip, sizeof ip);
    snprintf(name, sizeof name, "%s:%d(ul)", ip, ntohs(a.sin_port));
    int i = session_new(s, 0, -1, name);
    if (i < 0) { sock_close(s); return; }
    send_hello(i);
}

/* ======================= Otkrivanje cvorova (UDP broadcast) ======================= */
/* Nije deo konsenzusa ni TCP protokola. Svake 2 s cvor salje na UDP port (podrazumevano 7000):
 *   "PIXELCHAIN v1 <genesis hex> <tcp port> <identitet hex>"
 * Ko primi takvu poruku sa istim genesis-om, doda posiljaoca (njegova IP adresa + tcp port) u listu cvorova. */

static int g_tcp_port = 7001;
static int g_http_port = 0;
static char g_my_ip[32] = "";

static int discover_open(void)
{
    g_udp = socket(AF_INET, SOCK_DGRAM, 0);
    if (g_udp == BAD_SOCK) return -1;
    int one = 1;
    setsockopt(g_udp, SOL_SOCKET, SO_REUSEADDR, (const char *)&one, sizeof one);
#ifdef SO_REUSEPORT
    setsockopt(g_udp, SOL_SOCKET, SO_REUSEPORT, (const char *)&one, sizeof one);
#endif
    setsockopt(g_udp, SOL_SOCKET, SO_BROADCAST, (const char *)&one, sizeof one);
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons((uint16_t)g_discover_port);
    a.sin_addr.s_addr = htonl(INADDR_ANY);
    if (bind(g_udp, (struct sockaddr *)&a, sizeof a) != 0) { sock_close(g_udp); g_udp = BAD_SOCK; return -1; }
#ifdef _WIN32
    u_long nb = 1; ioctlsocket(g_udp, FIONBIO, &nb);
#else
    fcntl(g_udp, F_SETFL, fcntl(g_udp, F_GETFL, 0) | O_NONBLOCK);
#endif
    return 0;
}

static void discover_announce(void)
{
    if (g_udp == BAD_SOCK) return;
    char gh[65], idh[65], msg[200];
    to_hex(gh, g_genesis, 32);
    to_hex(idh, g_id.pk, 32);
    int n = snprintf(msg, sizeof msg, "PIXELCHAIN v1 %s %d %s", gh, g_tcp_port, idh);
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons((uint16_t)g_discover_port);
    a.sin_addr.s_addr = htonl(INADDR_BROADCAST);                     /* cela lokalna mreza */
    sendto(g_udp, msg, n, 0, (struct sockaddr *)&a, sizeof a);
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);                      /* i cvorovi na ovom racunaru */
    sendto(g_udp, msg, n, 0, (struct sockaddr *)&a, sizeof a);
}

static void discover_receive(void)
{
    for (int k = 0; k < 32; k++) {
        char buf[256];
        struct sockaddr_in from;
        socklen_t fl = sizeof from;
        int n = (int)recvfrom(g_udp, buf, sizeof buf - 1, 0, (struct sockaddr *)&from, &fl);
        if (n <= 0) return;
        buf[n] = 0;
        char gh[80], idh[80];
        int port;
        uint8_t gen[32], id[32];
        if (sscanf(buf, "PIXELCHAIN v1 %70s %d %70s", gh, &port, idh) != 3) continue;
        if (port <= 0 || port > 65535) continue;
        if (from_hex(gen, 32, gh, strlen(gh)) || memcmp(gen, g_genesis, 32)) continue;   /* drugi lanac */
        if (from_hex(id, 32, idh, strlen(idh)) || !key_ok(id)) continue;
        if (!memcmp(id, g_id.pk, 32)) continue;                                           /* to sam ja */
        int known = 0;
        for (int p = 0; p < g_npeers; p++) {
            if (g_peers[p].has_id && !memcmp(g_peers[p].id, id, 32)) { known = 1; break; }
            if (g_peers[p].addr.sin_addr.s_addr == from.sin_addr.s_addr && g_peers[p].port == port) {
                g_peers[p].has_id = 1; memcpy(g_peers[p].id, id, 32); known = 1; break;
            }
        }
        if (known || g_npeers >= MAX_PEERS) continue;
        Peer *p = &g_peers[g_npeers++];
        memset(p, 0, sizeof *p);
        p->session = -1;
        p->addr = from;
        p->addr.sin_port = htons((uint16_t)port);
        inet_ntop(AF_INET, &from.sin_addr, p->host, sizeof p->host);
        p->port = port;
        p->has_id = 1;
        memcpy(p->id, id, 32);
        logmsg("pronadjen cvor %s:%d (identitet %.12s...)", p->host, port, idh);
    }
}

static void http_fds(fd_set *rd, fd_set *wr, sock_t *maxfd, int *any);
static void http_io(fd_set *rd, fd_set *wr);

/* Jedan prolaz mreze: ceka najvise wait_ms na dogadjaje */
static void net_poll(int wait_ms)
{
    fd_set rd, wr, ex;
    FD_ZERO(&rd); FD_ZERO(&wr); FD_ZERO(&ex);
    sock_t maxfd = 0;
    int any = 0;
    if (g_listen != BAD_SOCK) { FD_SET(g_listen, &rd); maxfd = g_listen; any = 1; }
    if (g_udp != BAD_SOCK) { FD_SET(g_udp, &rd); if (g_udp > maxfd) maxfd = g_udp; any = 1; }
    http_fds(&rd, &wr, &maxfd, &any);
    for (int i = 0; i < MAX_SESSIONS; i++) {
        if (!g_s[i].used) continue;
        sock_t s = g_s[i].s;
        if (g_s[i].connecting) { FD_SET(s, &wr); FD_SET(s, &ex); }
        else { FD_SET(s, &rd); if (g_s[i].wlen) FD_SET(s, &wr); }
        if (s > maxfd) maxfd = s;
        any = 1;
    }
    struct timeval tv = { wait_ms / 1000, (wait_ms % 1000) * 1000 };
    if (!any) {
#ifdef _WIN32
        Sleep(wait_ms);
#else
        select(0, NULL, NULL, NULL, &tv);
#endif
        return;
    }
    int r = select((int)maxfd + 1, &rd, &wr, &ex, &tv);
    if (r < 0) return;
    if (g_listen != BAD_SOCK && FD_ISSET(g_listen, &rd)) accept_new();
    if (g_udp != BAD_SOCK && FD_ISSET(g_udp, &rd)) discover_receive();
    http_io(&rd, &wr);
    for (int i = 0; i < MAX_SESSIONS; i++) {
        if (!g_s[i].used) continue;
        sock_t s = g_s[i].s;
        if (g_s[i].connecting) {
            if (FD_ISSET(s, &ex)) session_close(i, NULL);
            else if (FD_ISSET(s, &wr)) connect_done(i);
            continue;
        }
        if (FD_ISSET(s, &rd)) session_read(i);
    }
    uint64_t now = now_ms();
    for (int i = 0; i < MAX_SESSIONS; i++) {
        if (!g_s[i].used) continue;
        if (g_s[i].connecting) {
            if (now - g_s[i].connect_start > CONNECT_TO_MS) session_close(i, NULL);
            continue;
        }
        if (now - g_s[i].last_rx > IDLE_MS) { session_close(i, "30 s bez poruka"); continue; }
        session_flush(i);
    }
    for (int p = 0; p < g_npeers; p++)
        if (g_peers[p].session < 0 && now >= g_peers[p].next_try) start_connect(p);
}

/* ======================= Periodicni poslovi (svake 2 s) ======================= */

static int g_mem_cursor = 0;

static void tick(void)
{
    for (int i = 0; i < MAX_SESSIONS; i++) {
        if (!g_s[i].used || g_s[i].connecting) continue;
        send_hello(i);                                     /* hello se ponavlja uvek */
        if (g_s[i].ready) { send_msg(i, "{\"type\":\"get_tip\"}"); send_tip(i); }
    }
    /* do 4 trazena hesa, rotirajuci, svim spremnim cvorovima */
    for (int k = 0; k < 4 && g_nwanted > 0; k++) {
        g_want_cursor %= g_nwanted;
        uint8_t *h = g_wanted[g_want_cursor++];
        if (is_known(h)) { wanted_remove(h); k--; continue; }
        for (int i = 0; i < MAX_SESSIONS; i++) if (g_s[i].used && g_s[i].ready) send_get_block(i, h);
    }
    /* do 4 transakcije iz mempool-a ponovo najavi */
    for (int k = 0; k < 4 && k < g_nmem; k++) {
        g_mem_cursor %= g_nmem;
        char buf[1024];
        make_tx_msg(&g_mem[g_mem_cursor++], buf, sizeof buf);
        broadcast(buf);
    }
    /* blokovi iz buducnosti koji su sada prihvatljivi */
    after_new_blocks();
}

/* ======================= Rudarenje (u delovima, izmedju mreze) ======================= */

static int      g_mining = 0;
static Block    g_cand;
static Tx       g_cand_tx[MAX_TX];
static uint8_t  g_cand_hdr[HEADER_LEN];
static uint64_t g_cand_tip_gen = (uint64_t)-1, g_cand_mem_gen = (uint64_t)-1;
static uint64_t g_cand_nonce, g_cand_start_ms, g_cand_tries;
static double   g_hashrate = 0, g_last_secs = 0, g_total_secs = 0;
static long     g_mined = 0;

static void build_candidate(void)
{
    memset(&g_cand, 0, sizeof g_cand);
    const Block *tip = &g_blocks[g_tip];
    memcpy(g_cand.prev, tip->hash, 32);
    g_cand.height = tip->height + 1;
    memcpy(g_cand.miner, g_id.pk, 32);
    g_cand.time = unix_now();
    if (g_cand.time < tip->time) g_cand.time = tip->time;
    static State st;
    st = g_tip_state;
    int n = 0;
    for (int i = 0; i < g_nmem && n < MAX_TX; i++)
        if (tx_apply(&st, &g_mem[i], 0) == 0) g_cand_tx[n++] = g_mem[i];
    g_cand.ntx = (uint16_t)n;
    g_cand.tx = g_cand_tx;
    compute_commit(g_cand_tx, n, g_cand.commit);
    block_header(&g_cand, g_cand_hdr);
    g_cand_nonce = 0;
    if (g_cand_tip_gen != g_tip_gen) { g_cand_start_ms = now_ms(); g_cand_tries = 0; }
    g_cand_tip_gen = g_tip_gen;
    g_cand_mem_gen = g_mem_gen;
}

static void mine_slice(void)
{
    if (g_cand_tip_gen != g_tip_gen || g_cand_mem_gen != g_mem_gen) build_candidate();
    uint8_t h[32];
    uint64_t t0 = now_ms();
    for (int k = 0; k < MINE_SLICE; k++) {
        put_u64(g_cand_hdr + NONCE_OFFSET, g_cand_nonce);
        crypto_hash_sha256(h, g_cand_hdr, HEADER_LEN);
        g_cand_tries++;
        if (leading_zero_bits(h) >= DIFFICULTY) {
            double secs = (now_ms() - g_cand_start_ms) / 1000.0;
            g_last_secs = secs; g_total_secs += secs; g_mined++;
            g_cand.nonce = g_cand_nonce;
            Block b = g_cand;
            logmsg("IZRUDAREN blok %llu za %.1f s (%u tx)", (unsigned long long)b.height, secs, b.ntx);
            handle_block(&b, -1);
            g_cand_tip_gen = (uint64_t)-1;
            return;
        }
        g_cand_nonce++;
    }
    uint64_t dt = now_ms() - t0;
    if (dt > 0) g_hashrate = 0.8 * g_hashrate + 0.2 * (MINE_SLICE * 1000.0 / dt);
}

/* ======================= Automatske transakcije (demo) ======================= */

static int g_demo = 1;
static char g_in[256];
static int  g_inlen;
static int  g_console;
static Key g_key_b;                   /* drugi kljuc, za prenose */

static void demo_tx(void)
{
    static int mine_p[NPARCELS];
    int n = 0;
    for (int i = 0; i < NPARCELS; i++) {
        const Parcel *p = &g_mem_state.p[i];
        if (p->owned && (!memcmp(p->owner, g_id.pk, 32) || !memcmp(p->owner, g_key_b.pk, 32))) mine_p[n++] = i;
    }
    if (n == 0) return;
    int pi = mine_p[randombytes_uniform(n)];
    const Parcel *p = &g_mem_state.p[pi];
    const Key *k = !memcmp(p->owner, g_id.pk, 32) ? &g_id : &g_key_b;
    const Key *other = (k == &g_id) ? &g_key_b : &g_id;
    Tx t;
    memset(&t, 0, sizeof t);
    t.parcel = (uint16_t)pi;
    t.counter = p->counter;
    if (randombytes_uniform(5) == 0) { t.action = ACT_TRANSFER; memcpy(t.recipient, other->pk, 32); }
    else { t.action = ACT_PAINT; randombytes_buf(t.color, 3); }
    tx_sign(&t, k);
    handle_tx(&t, -1);
    if (t.action == ACT_PAINT)
        logmsg("moja tx: bojenje parcele %d (%d,%d) u #%02x%02x%02x", pi, pi % 64, pi / 64, t.color[0], t.color[1], t.color[2]);
    else
        logmsg("moja tx: prenos parcele %d na kljuc %s", pi, other == &g_id ? "A" : "B");
}

/* ======================= Prikaz ======================= */

static char g_data_dir[256] = "data";
static char g_listen_addr[64] = "0.0.0.0:7001";

static void parcel_rgb(const State *st, int p, uint8_t c[3]) { memcpy(c, st->p[p].color, 3); }

static void save_bmp(const State *st)
{
    char path[300];
    snprintf(path, sizeof path, "%s/canvas.bmp", g_data_dir);
    const int S = 10, W = CANVAS_W * S, H = CANVAS_H * S, row = W * 3;
    uint32_t data = (uint32_t)(row * H), size = 54 + data;
    uint8_t hdr[54] = {0};
    hdr[0] = 'B'; hdr[1] = 'M'; memcpy(hdr + 2, &size, 4); hdr[10] = 54; hdr[14] = 40;
    memcpy(hdr + 18, &W, 4); memcpy(hdr + 22, &H, 4); hdr[26] = 1; hdr[28] = 24; memcpy(hdr + 34, &data, 4);
    FILE *f = fopen(path, "wb");
    if (!f) return;
    fwrite(hdr, 1, 54, f);
    uint8_t *line = malloc(row);
    for (int py = H - 1; py >= 0; py--) {
        for (int px = 0; px < W; px++) {
            uint8_t c[3];
            parcel_rgb(st, (py / S) * CANVAS_W + px / S, c);
            line[px*3] = c[2]; line[px*3+1] = c[1]; line[px*3+2] = c[0];
        }
        fwrite(line, 1, row, f);
    }
    free(line);
    fclose(f);
}

/* Ime rudara za prikaz: "JA", adresa povezanog cvora sa tim identitetom, ili pocetak kljuca */
static void miner_label(const uint8_t pk[32], char *out, size_t cap)
{
    char hx[65];
    to_hex(hx, pk, 32);
    if (!memcmp(pk, g_id.pk, 32)) { snprintf(out, cap, "JA (%.8s)", hx); return; }
    for (int i = 0; i < MAX_SESSIONS; i++)
        if (g_s[i].used && g_s[i].ready && !memcmp(g_s[i].identity, pk, 32)) {
            char nm[64];
            snprintf(nm, sizeof nm, "%s", g_s[i].name);
            char *p = strstr(nm, "(ul)"); if (p) *p = 0;      /* dolazna veza: port je nasumican, ostavi IP */
            if (strstr(g_s[i].name, "(ul)")) { char *c = strrchr(nm, ':'); if (c) *c = 0; }
            snprintf(out, cap, "%s (%.8s)", nm, hx);
            return;
        }
    snprintf(out, cap, "%.8s...", hx);
}

/* Poslednji blokovi na glavnom lancu i koliko je ko izrudario */
static void draw_blocks(void)
{
    typedef struct { uint8_t pk[32]; int n; } Tally;
    static Tally t[32];
    int nt = 0;
    for (int i = g_tip; i > 0; i = g_blocks[i].parent) {
        int k;
        for (k = 0; k < nt; k++) if (!memcmp(t[k].pk, g_blocks[i].miner, 32)) break;
        if (k == nt) { if (nt == 32) continue; memcpy(t[nt].pk, g_blocks[i].miner, 32); t[nt].n = 0; nt++; }
        t[k].n++;
    }
    char lab[96];
    printf("Ko je koliko blokova izrudario (glavni lanac):\n");
    if (nt == 0) printf("  jos nema blokova\n");
    for (int k = 0; k < nt; k++) { miner_label(t[k].pk, lab, sizeof lab); printf("  %-34s %d\n", lab, t[k].n); }
    printf("\nPoslednji blokovi:\n");
    printf("  %-6s %-14s %-34s %-4s %s\n", "visina", "hes", "rudar", "tx", "nagrada");
    int shown = 0;
    for (int i = g_tip; i > 0 && shown < 8; i = g_blocks[i].parent, shown++) {
        const Block *b = &g_blocks[i];
        char hx[65], rw[32];
        to_hex(hx, b->hash, 32);
        miner_label(b->miner, lab, sizeof lab);
        if (b->reward >= 0) snprintf(rw, sizeof rw, "%d (%d,%d)", b->reward, b->reward % 64, b->reward / 64);
        else snprintf(rw, sizeof rw, "nema");
        printf("  %-6llu %.12s.. %-34s %-4u %s\n", (unsigned long long)b->height, hx, lab, b->ntx, rw);
    }
    printf("\n");
}

static void draw_screen(void)
{
    const State *st = &g_tip_state;
    const Block *tip = &g_blocks[g_tip];
    char th[65], sh[65], idh[65];
    uint8_t shash[32];
    state_hash(st, shash);
    to_hex(th, tip->hash, 32);
    to_hex(sh, shash, 32);
    to_hex(idh, g_id.pk, 32);
    int owned = 0, mine_a = 0, ready = 0, sessions = 0;
    for (int i = 0; i < NPARCELS; i++) if (st->p[i].owned) { owned++; if (!memcmp(st->p[i].owner, g_id.pk, 32)) mine_a++; }
    for (int i = 0; i < MAX_SESSIONS; i++) if (g_s[i].used) { sessions++; if (g_s[i].ready) ready++; }

    printf("\x1b[H\x1b[2J");
    printf("Pixel Chain cvor   slusam %s   identitet %.16s...\n", g_listen_addr, idh);
    printf("Cvorovi iz configa/pronadjeni: %d   ", g_npeers);
    printf("Veze: %d spremnih / %d ukupno   lag %s   rudarenje %s",
           ready, sessions, g_lag ? "UKLJUCEN" : "iskljucen", g_mining ? "UKLJUCENO" : "iskljuceno");
    if (g_mining) printf("   %.0f heseva/s", g_hashrate);
    printf("\n");
    if (g_mining && g_mined > 0)
        printf("Moji blokovi: %ld   poslednji %.1f s   prosek %.1f s   ocekivano ~%.1f s (samo ja)\n",
               g_mined, g_last_secs, g_total_secs / g_mined, g_hashrate > 0 ? (double)(1ULL << DIFFICULTY) / g_hashrate : 0);
    if (g_my_ip[0]) printf("MOJA ADRESA: %s:%d  (ovo kolega upisuje u svoj config.txt kao peers=...)\n", g_my_ip, g_tcp_port);
    if (g_http_port) printf("INTERFEJS: otvori http://localhost:%d u browseru\n", g_http_port);
    printf("VRH:   visina %llu   hes %.20s...\n", (unsigned long long)tip->height, th);
    printf("STANJE (uporedi sa drugim cvorom): %.20s...\n", sh);
    printf("Parcele: %d/%d zauzeto (moje: %d)   mempool %d   na cekanju %d   trazim %d\n\n",
           owned, NPARCELS, mine_a, g_nmem, g_npending, g_nwanted);
    for (int y = 0; y < CANVAS_H; y += 2) {
        for (int x = 0; x < CANVAS_W; x++) {
            uint8_t a[3], b[3];
            parcel_rgb(st, y * CANVAS_W + x, a);
            parcel_rgb(st, (y + 1) * CANVAS_W + x, b);
            /* parcele bez vlasnika su crne; da se vide na crnom ekranu, prikazujemo ih tamnosivo */
            if (!st->p[y * CANVAS_W + x].owned) a[0] = a[1] = a[2] = 30;
            if (!st->p[(y + 1) * CANVAS_W + x].owned) b[0] = b[1] = b[2] = 30;
            printf("\x1b[38;2;%d;%d;%dm\x1b[48;2;%d;%d;%dm\xe2\x96\x80", a[0], a[1], a[2], b[0], b[1], b[2]);
        }
        printf("\x1b[0m\n");
    }
    printf("\n");
    draw_blocks();
    for (int i = 0; i < g_nlog; i++) printf("%s\n", g_log[i]);
    printf("\nKomande: paint x,y ff0000 | transfer x,y B | mine on/off | parcels | help | quit\n");
    if (g_console) printf("> %.*s", g_inlen, g_in);
    fflush(stdout);
}

/* ======================= Kljucevi na disku ======================= */

static void key_load_or_create(Key *k, const char *file)
{
    char path[300], line[128] = {0};
    uint8_t seed[32];
    snprintf(path, sizeof path, "%s/%s", g_data_dir, file);
    FILE *f = fopen(path, "r");
    if (f && fgets(line, sizeof line, f)) {
        size_t n = strlen(line);
        if (n && line[n-1] == '\n') line[--n] = 0;
        if (from_hex(seed, 32, line, n) == 0) { fclose(f); key_from_seed(k, seed); return; }
    }
    if (f) fclose(f);
    randombytes_buf(seed, 32);
    key_from_seed(k, seed);
    f = fopen(path, "w");
    if (f) { char hx[65]; to_hex(hx, seed, 32); fprintf(f, "%s\n", hx); fclose(f); }
}

/* ======================= Samoprovera ======================= */

static int g_fail = 0;
static void check(int ok, const char *what) { printf("  [%s] %s\n", ok ? " OK " : "GRESKA", what); if (!ok) g_fail++; }

static int self_test(void)
{
    uint8_t h[32], seed[32];
    char hx[129];
    printf("Samoprovera:\n");
    crypto_hash_sha256(h, (const uint8_t *)"abc", 3);
    to_hex(hx, h, 32);
    check(!strcmp(hx, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"), "SHA-256(\"abc\")");
    Key rfc;
    from_hex(seed, 32, "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60", 64);
    key_from_seed(&rfc, seed);
    to_hex(hx, rfc.pk, 32);
    check(!strcmp(hx, "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"), "Ed25519 RFC 8032 test 1");
    Block g; make_genesis(&g); to_hex(hx, g.hash, 32);
    check(!strcmp(hx, GENESIS_HEX), "genesis hes iz specifikacije");

    Key A, B;
    memset(seed, 1, 32); key_from_seed(&A, seed);
    memset(seed, 2, 32); key_from_seed(&B, seed);
    static State st;
    memset(&st, 0, sizeof st);
    st.p[5].owned = 1; memcpy(st.p[5].owner, A.pk, 32);
    Tx t; memset(&t, 0, sizeof t);
    t.action = ACT_PAINT; t.parcel = 5; t.counter = 0; t.color[0] = 255; tx_sign(&t, &A);
    check(tx_apply(&st, &t, 1) == 0, "vlasnik boji (brojac 0)");
    check(tx_apply(&st, &t, 1) == 5, "ista transakcija drugi put: odbijena");
    Tx t2 = t; t2.counter = 1; t2.color[1] = 9;
    check(tx_apply(&st, &t2, 1) == 2, "izmenjena posle potpisa: los potpis");
    Tx t3; memset(&t3, 0, sizeof t3); t3.action = ACT_PAINT; t3.parcel = 5; t3.counter = 1; tx_sign(&t3, &B);
    check(tx_apply(&st, &t3, 1) == 4, "tudja parcela: odbijeno");
    Tx t4; memset(&t4, 0, sizeof t4); t4.action = ACT_TRANSFER; t4.parcel = 5; t4.counter = 1; memcpy(t4.recipient, B.pk, 32); tx_sign(&t4, &A);
    check(tx_apply(&st, &t4, 1) == 0 && st.p[5].color[0] == 255, "prenos A->B, boja ostaje");
    Tx t5 = t; t5.counter = 2; tx_sign(&t5, &A);
    check(tx_apply(&st, &t5, 1) == 4, "posle prenosa A ne moze da boji");
    Tx t6; memset(&t6, 0, sizeof t6); t6.action = ACT_TRANSFER; t6.parcel = 5; t6.counter = 2; memset(t6.recipient, 0, 32); tx_sign(&t6, &B);
    check(tx_apply(&st, &t6, 1) == 3, "prenos na neispravan kljuc: odbijen");

    static State full;
    memset(&full, 0, sizeof full);
    uint8_t hh[32] = {0};
    hh[30] = 0x00; hh[31] = 0xb5;                          /* p0 = 181 */
    check(reward_parcel(&full, hh, 0) == 181, "nagrada: prva parcela iz hesa");
    full.p[181].owned = 1;
    check(reward_parcel(&full, hh, 0) == 0, "zauzeta 181, vreme 0: sledeca je 0");
    full.p[0].owned = 1;
    check(reward_parcel(&full, hh, 0) == -1, "ciklus na 0: nema nagrade");

    char js[2048];
    tx_to_json(&t4, js, sizeof js);
    Tx back;
    int r = json_parse(js, strlen(js));
    check(r >= 0 && json_to_tx(r, &back) == 0 && !memcmp(&back, &t4, sizeof back), "JSON transakcije tamo i nazad");
    check(json_parse("{\"type\":\"get_tip\",}", 19) < 0, "los JSON se odbija");
    printf("%s\n\n", g_fail ? "Samoprovera NIJE prosla!" : "Samoprovera prosla.");
    return g_fail;
}

/* ======================= Test podaci ======================= */

static void print_vectors(void)
{
    uint8_t seed[32], buf[151], id[32];
    char hx[400];
    Key A, B;
    memset(seed, 1, 32); key_from_seed(&A, seed);
    memset(seed, 2, 32); key_from_seed(&B, seed);
    to_hex(hx, A.pk, 32); printf("A javni (seed 01..01): %s\n", hx);
    to_hex(hx, B.pk, 32); printf("B javni (seed 02..02): %s\n", hx);
    Block g; make_genesis(&g);
    uint8_t hdr[HEADER_LEN]; block_header(&g, hdr);
    to_hex(hx, hdr, HEADER_LEN); printf("genesis zaglavlje: %s\n", hx);
    to_hex(hx, g.hash, 32);      printf("genesis hes:       %s\n", hx);
    Tx p; memset(&p, 0, sizeof p); p.action = ACT_PAINT; p.parcel = 181; p.counter = 0;
    p.color[0] = 0xff; p.color[1] = 0x80; p.color[2] = 0x00; tx_sign(&p, &A);
    size_t n = tx_canon(&p, buf); to_hex(hx, buf, n); printf("bojenje kanonska (%zu B): %s\n", n, hx);
    tx_id(&p, id); to_hex(hx, id, 32); printf("bojenje id: %s\n", hx);
    Tx t; memset(&t, 0, sizeof t); t.action = ACT_TRANSFER; t.parcel = 181; t.counter = 1;
    memcpy(t.recipient, B.pk, 32); tx_sign(&t, &A);
    n = tx_canon(&t, buf); to_hex(hx, buf, n); printf("prenos kanonski (%zu B): %s\n", n, hx);
    tx_id(&t, id); to_hex(hx, id, 32); printf("prenos id: %s\n", hx);
    Tx two[2] = { p, t };
    compute_commit(two, 2, id); to_hex(hx, id, 32); printf("commitment [bojenje, prenos]: %s\n", hx);
}

/* ======================= Komande iz terminala ======================= */

static int  g_quit = 0;

#ifndef _WIN32
static struct termios g_tio_old;
static int g_tio_saved = 0;
static void console_restore(void) { if (g_tio_saved) tcsetattr(0, TCSANOW, &g_tio_old); }
#endif

static void console_init(void)
{
    g_console = 1;
#ifndef _WIN32
    if (isatty(0) && tcgetattr(0, &g_tio_old) == 0) {
        struct termios t = g_tio_old;
        t.c_lflag &= ~(ICANON | ECHO);
        t.c_cc[VMIN] = 0; t.c_cc[VTIME] = 0;
        tcsetattr(0, TCSANOW, &t);
        g_tio_saved = 1;
        atexit(console_restore);
    }
    fcntl(0, F_SETFL, fcntl(0, F_GETFL, 0) | O_NONBLOCK);
#endif
}

static void draw_prompt(void)
{
    printf("\r\x1b[2K> %.*s", g_inlen, g_in);
    fflush(stdout);
}

/* Parcela iz "123" ili "x,y" */
static int parse_parcel(const char *s)
{
    int x, y;
    char *end;
    if (strchr(s, ',')) {
        if (sscanf(s, "%d,%d", &x, &y) != 2 || x < 0 || x >= CANVAS_W || y < 0 || y >= CANVAS_H) return -1;
        return y * CANVAS_W + x;
    }
    long v = strtol(s, &end, 10);
    if (*end || v < 0 || v >= NPARCELS) return -1;
    return (int)v;
}

/* Ko od mojih kljuceva (A = identitet, B) poseduje parcelu u stanju vrh + mempool */
static const Key *my_owner_key(int p)
{
    const Parcel *pp = &g_mem_state.p[p];
    if (!pp->owned) return NULL;
    if (!memcmp(pp->owner, g_id.pk, 32)) return &g_id;
    if (!memcmp(pp->owner, g_key_b.pk, 32)) return &g_key_b;
    return NULL;
}

/* Boja iz "ff0000" ili "#FF0000" */
static int parse_color(const char *a, uint8_t rgb[3])
{
    char col[7];
    if (a[0] == '#') a++;
    if (strlen(a) != 6) return -1;
    for (int i = 0; i < 6; i++) col[i] = (a[i] >= 'A' && a[i] <= 'F') ? a[i] + 32 : a[i];
    col[6] = 0;
    return from_hex(rgb, 3, col, 6);
}

/* Oboji moju parcelu: napravi, potpisi i posalji transakciju. Vraca poruku za prikaz. */
static const char *paint_parcel(int p, const uint8_t rgb[3])
{
    static char msg[160];
    const Key *k = my_owner_key(p);
    if (!k) { snprintf(msg, sizeof msg, "parcela %d (%d,%d) nije tvoja", p, p % 64, p / 64); return msg; }
    Tx t;
    memset(&t, 0, sizeof t);
    t.action = ACT_PAINT;
    t.parcel = (uint16_t)p;
    t.counter = g_mem_state.p[p].counter;
    memcpy(t.color, rgb, 3);
    tx_sign(&t, k);
    int before = g_nmem;
    handle_tx(&t, -1);
    if (g_nmem > before)
        snprintf(msg, sizeof msg, "poslato: bojenje parcele %d (%d,%d) u #%02x%02x%02x - videce se kad udje u blok",
                 p, p % 64, p / 64, rgb[0], rgb[1], rgb[2]);
    else snprintf(msg, sizeof msg, "bojenje parcele %d odbijeno", p);
    return msg;
}

static void cmd_help(void)
{
    logmsg("komande: paint <parcela|x,y> <rrggbb>   transfer <parcela|x,y> <A|B|kljuc>");
    logmsg("         mine on|off   demo on|off   parcels   connect <ip:port>   quit");
}

static void run_command(char *line)
{
    char *argv[8];
    int argc = 0;
    for (char *t = strtok(line, " \t"); t && argc < 8; t = strtok(NULL, " \t")) argv[argc++] = t;
    if (argc == 0) return;
    const char *c = argv[0];

    if (!strcmp(c, "help") || !strcmp(c, "?")) { cmd_help(); return; }
    if (!strcmp(c, "quit") || !strcmp(c, "exit")) { g_quit = 1; return; }

    if (!strcmp(c, "mine") && argc == 2) {
        g_mining = !strcmp(argv[1], "on");
        g_cand_tip_gen = (uint64_t)-1;
        logmsg("rudarenje %s", g_mining ? "UKLJUCENO" : "iskljuceno");
        return;
    }
    if (!strcmp(c, "demo") && argc == 2) {
        g_demo = !strcmp(argv[1], "on");
        logmsg("automatske transakcije %s", g_demo ? "ukljucene" : "iskljucene");
        return;
    }
    if (!strcmp(c, "parcels")) {
        char buf[200]; int n = 0, cnt = 0;
        buf[0] = 0;
        for (int p = 0; p < NPARCELS; p++) {
            const Key *k = my_owner_key(p);
            if (!k) continue;
            cnt++;
            if (n < 150) n += snprintf(buf + n, sizeof buf - n, "%d(%d,%d)%s ", p, p % 64, p / 64, k == &g_key_b ? "B" : "");
        }
        logmsg("moje parcele (%d): %s%s", cnt, cnt ? buf : "nemam jos nijednu - ukljuci rudarenje", n >= 150 ? "..." : "");
        return;
    }
    if (!strcmp(c, "connect") && argc == 2) {
        char full[96];
        if (!strchr(argv[1], ':')) snprintf(full, sizeof full, "127.0.0.1:%s", argv[1]);
        else snprintf(full, sizeof full, "%s", argv[1]);
        if (g_npeers >= MAX_PEERS) { logmsg("previse cvorova"); return; }
        Peer p; memset(&p, 0, sizeof p); p.session = -1;
        if (parse_addr(full, &p.addr, p.host, &p.port)) { logmsg("losa adresa: %s", full); return; }
        g_peers[g_npeers++] = p;
        logmsg("povezujem se na %s", full);
        return;
    }
    if ((!strcmp(c, "paint") || !strcmp(c, "transfer")) && argc == 3) {
        int p = parse_parcel(argv[1]);
        if (p < 0) { logmsg("losa parcela: %s (broj 0-2047 ili x,y)", argv[1]); return; }
        if (!strcmp(c, "paint")) {
            uint8_t rgb[3];
            if (parse_color(argv[2], rgb)) { logmsg("boja mora biti 6 hex znakova, npr. ff0000"); return; }
            logmsg("%s", paint_parcel(p, rgb));
            return;
        }
        const Key *k = my_owner_key(p);
        if (!k) { logmsg("parcela %d (%d,%d) nije moja", p, p % 64, p / 64); return; }
        Tx t;
        memset(&t, 0, sizeof t);
        t.parcel = (uint16_t)p;
        t.counter = g_mem_state.p[p].counter;
        t.action = ACT_TRANSFER;
        if (!strcmp(argv[2], "A")) memcpy(t.recipient, g_id.pk, 32);
        else if (!strcmp(argv[2], "B")) memcpy(t.recipient, g_key_b.pk, 32);
        else if (from_hex(t.recipient, 32, argv[2], strlen(argv[2]))) { logmsg("los kljuc primaoca (64 hex, mala slova, ili A/B)"); return; }
        tx_sign(&t, k);
        int before = g_nmem;
        handle_tx(&t, -1);
        logmsg(g_nmem > before ? "poslato: prenos parcele %d" : "prenos parcele %d odbijen", p);
        return;
    }
    logmsg("nepoznata komanda: %s (ukucaj help)", c);
}

/* Procitaj sve pritisnute tastere bez cekanja */
static void console_poll(void)
{
    for (;;) {
        int ch;
#ifdef _WIN32
        if (!_kbhit()) return;
        ch = _getch();
        if (ch == 0 || ch == 224) { _getch(); continue; }       /* strelice i slicno */
#else
        unsigned char b;
        if (read(0, &b, 1) != 1) return;
        ch = b;
#endif
        if (ch == '\r' || ch == '\n') {
            g_in[g_inlen] = 0;
            char line[256];
            snprintf(line, sizeof line, "%s", g_in);
            g_inlen = 0;
            if (line[0]) { logmsg("> %s", line); run_command(line); }
            draw_screen();
        } else if (ch == 8 || ch == 127) {
            if (g_inlen > 0) g_inlen--;
        } else if (ch == 3) {
            g_quit = 1;
            return;
        } else if (ch >= 32 && ch < 127 && g_inlen < (int)sizeof g_in - 1) {
            g_in[g_inlen++] = (char)ch;
        }
        draw_prompt();
    }
}

/* ======================= Interfejs u browseru (HTTP) ======================= */
/* http://localhost:<tcp port + 1000>  - samo na ovom racunaru (127.0.0.1) */

static const char UI_HTML[] =
    "<!DOCTYPE html>\n"
    "<html lang=\"sr\"><head><meta charset=\"utf-8\"><title>Pixel Chain</title>\n"
    "<style>\n"
    "body{font-family:system-ui,sans-serif;background:#111;color:#eee;margin:20px}\n"
    "h1{font-size:20px;margin:0 0 4px}\n"
    ".info{color:#aaa;font-size:13px;margin-bottom:12px}\n"
    ".wrap{display:flex;gap:24px;align-items:flex-start;flex-wrap:wrap}\n"
    "canvas{border:1px solid #444;cursor:pointer;image-rendering:pixelated}\n"
    ".panel{background:#1c1c1c;border:1px solid #333;border-radius:8px;padding:14px;width:300px}\n"
    ".panel h2{font-size:15px;margin:0 0 10px}\n"
    "button{background:#3a7bd5;color:#fff;border:0;border-radius:6px;padding:8px 14px;font-size:14px;cursor:pointer}\n"
    "button:disabled{background:#444;cursor:default}\n"
    "input[type=color]{width:60px;height:36px;border:0;background:none;vertical-align:middle}\n"
    ".msg{margin-top:8px;font-size:13px;color:#8f8}\n"
    ".list{display:flex;flex-wrap:wrap;gap:4px;margin-top:6px}\n"
    ".sw{width:22px;height:22px;border:2px solid #666;border-radius:3px;cursor:pointer}\n"
    ".sw.sel{border-color:#ff0}\n"
    "table{border-collapse:collapse;font-size:12px;width:100%}\n"
    "td{padding:3px 4px;border-bottom:1px solid #2a2a2a}\n"
    ".me{color:#ff0}\n"
    "</style></head><body>\n"
    "<h1>Pixel Chain</h1>\n"
    "<div class=\"info\" id=\"info\">ucitavam...</div>\n"
    "<div class=\"wrap\">\n"
    "  <canvas id=\"c\" width=\"640\" height=\"320\"></canvas>\n"
    "  <div class=\"panel\">\n"
    "    <h2>Oboji svoju parcelu</h2>\n"
    "    <div id=\"selinfo\">Klikni na svoju parcelu (zuti okvir) na platnu ili ispod.</div>\n"
    "    <div style=\"margin-top:10px\"><input type=\"color\" id=\"col\" value=\"#ff0000\"> <button id=\"go\" disabled>Oboji</button></div>\n"
    "    <div class=\"msg\" id=\"msg\"></div>\n"
    "    <h2 style=\"margin-top:16px\">Moje parcele</h2>\n"
    "    <div class=\"list\" id=\"mine\"></div>\n"
    "  </div>\n"
    "  <div class=\"panel\">\n"
    "    <h2>Poslednji blokovi</h2>\n"
    "    <table id=\"blocks\"></table>\n"
    "  </div>\n"
    "</div>\n"
    "<script>\n"
    "const S=10, W=64, H=32;\n"
    "const cv=document.getElementById('c'), ctx=cv.getContext('2d');\n"
    "let st=null, sel=-1;\n"
    "function hex(i){ return '#'+st.colors.substr(i*6,6); }\n"
    "function draw(){\n"
    "  for(let i=0;i<W*H;i++){\n"
    "    const x=i%W, y=Math.floor(i/W), o=st.owner[i];\n"
    "    ctx.fillStyle = o=='0' ? '#1e1e1e' : hex(i);\n"
    "    ctx.fillRect(x*S,y*S,S,S);\n"
    "    if(o=='2'||o=='3'){ ctx.strokeStyle='#ff0'; ctx.lineWidth=1; ctx.strokeRect(x*S+0.5,y*S+0.5,S-1,S-1); }\n"
    "    else if(o=='1'){ ctx.strokeStyle='#666'; ctx.lineWidth=1; ctx.strokeRect(x*S+0.5,y*S+0.5,S-1,S-1); }\n"
    "  }\n"
    "  if(sel>=0){ ctx.strokeStyle='#f0f'; ctx.lineWidth=2; ctx.strokeRect((sel%W)*S+1,Math.floor(sel/W)*S+1,S-2,S-2); }\n"
    "}\n"
    "function side(){\n"
    "  document.getElementById('info').textContent =\n"
    "    'visina '+st.height+'   vrh '+st.tip.substr(0,16)+'...   veze: '+st.peers+'   rudarenje: '+(st.mining?'ukljuceno':'iskljuceno');\n"
    "  const m=document.getElementById('mine'); m.innerHTML='';\n"
    "  let n=0;\n"
    "  for(let i=0;i<W*H;i++) if(st.owner[i]=='2'||st.owner[i]=='3'){\n"
    "    const d=document.createElement('div'); d.className='sw'+(i==sel?' sel':''); d.style.background=hex(i);\n"
    "    d.title='parcela '+i+' ('+(i%W)+','+Math.floor(i/W)+')'; d.onclick=()=>pick(i); m.appendChild(d); n++;\n"
    "  }\n"
    "  if(!n) m.textContent='Jos nemas parcelu. Sacekaj da izrudaris blok.';\n"
    "  const t=document.getElementById('blocks'); t.innerHTML='<tr><td>visina</td><td>rudar</td><td>parcela</td></tr>';\n"
    "  for(const b of st.blocks){\n"
    "    const r=document.createElement('tr');\n"
    "    r.innerHTML='<td>'+b.height+'</td><td class=\"'+(b.me?'me':'')+'\">'+b.miner+'</td><td>'+(b.reward<0?'nema':b.reward+' ('+(b.reward%W)+','+Math.floor(b.reward/W)+')')+'</td>';\n"
    "    if(b.me && b.reward>=0){ r.style.cursor='pointer'; r.onclick=()=>pick(b.reward); }\n"
    "    t.appendChild(r);\n"
    "  }\n"
    "  const ok = sel>=0 && (st.owner[sel]=='2'||st.owner[sel]=='3');\n"
    "  document.getElementById('go').disabled=!ok;\n"
    "  document.getElementById('selinfo').textContent = sel<0 ? 'Klikni na svoju parcelu (zuti okvir) na platnu ili ispod.'\n"
    "    : 'Parcela '+sel+' (x='+(sel%W)+', y='+Math.floor(sel/W)+')'+(ok?'':' nije tvoja');\n"
    "}\n"
    "function pick(i){ sel=i; draw(); side(); }\n"
    "cv.onclick=e=>{ const r=cv.getBoundingClientRect(); const x=Math.floor((e.clientX-r.left)/S), y=Math.floor((e.clientY-r.top)/S); pick(y*W+x); };\n"
    "document.getElementById('go').onclick=async()=>{\n"
    "  const c=document.getElementById('col').value.substr(1);\n"
    "  const r=await fetch('/paint?p='+sel+'&c='+c); const j=await r.json();\n"
    "  document.getElementById('msg').textContent=j.msg;\n"
    "};\n"
    "async function load(){ try{ const r=await fetch('/state'); st=await r.json(); draw(); side(); }catch(e){ document.getElementById('info').textContent='cvor ne radi'; } }\n"
    "setInterval(load,1000); load();\n"
    "</script></body></html>\n"
    "\n";

#define MAX_HTTP 8
typedef struct { int used; sock_t s; char req[2048]; int rlen; char *out; size_t olen, osent; uint64_t start; } HttpConn;
static HttpConn g_http[MAX_HTTP];
static sock_t   g_http_listen = BAD_SOCK;

static int http_open(int port)
{
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons((uint16_t)port);
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    g_http_listen = socket(AF_INET, SOCK_STREAM, 0);
    if (g_http_listen == BAD_SOCK) return -1;
    int one = 1;
    setsockopt(g_http_listen, SOL_SOCKET, SO_REUSEADDR, (const char *)&one, sizeof one);
    if (bind(g_http_listen, (struct sockaddr *)&a, sizeof a) != 0 || listen(g_http_listen, 8) != 0) {
        sock_close(g_http_listen); g_http_listen = BAD_SOCK; return -1;
    }
    set_nonblock(g_http_listen);
    g_http_port = port;
    return 0;
}

static void http_reply(HttpConn *h, const char *type, const char *body, size_t blen)
{
    char head[256];
    int n = snprintf(head, sizeof head,
        "HTTP/1.1 200 OK\r\nContent-Type: %s\r\nContent-Length: %zu\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", type, blen);
    h->out = malloc(n + blen);
    memcpy(h->out, head, n);
    memcpy(h->out + n, body, blen);
    h->olen = n + blen;
    h->osent = 0;
}

static int query_get(const char *q, const char *key, char *out, size_t cap)
{
    size_t kl = strlen(key);
    for (const char *p = q; p && *p; ) {
        if (!strncmp(p, key, kl) && p[kl] == '=') {
            p += kl + 1;
            size_t n = 0;
            while (*p && *p != '&' && *p != ' ' && n + 1 < cap) out[n++] = *p++;
            out[n] = 0;
            return 0;
        }
        p = strchr(p, '&');
        if (p) p++;
    }
    return -1;
}

static void http_state(HttpConn *h)
{
    static char body[64 * 1024];
    const State *st = &g_tip_state;
    char th[65];
    to_hex(th, g_blocks[g_tip].hash, 32);
    int ready = 0;
    for (int i = 0; i < MAX_SESSIONS; i++) if (g_s[i].used && g_s[i].ready) ready++;
    int n = snprintf(body, sizeof body, "{\"height\":%llu,\"tip\":\"%s\",\"peers\":%d,\"mining\":%s,\"colors\":\"",
                     (unsigned long long)g_blocks[g_tip].height, th, ready, g_mining ? "true" : "false");
    for (int i = 0; i < NPARCELS; i++) { to_hex(body + n, st->p[i].color, 3); n += 6; }
    n += snprintf(body + n, sizeof body - n, "\",\"owner\":\"");
    /* 0 = niciji, 1 = tudji, 2 = moj (A), 3 = moj (B); vlasnistvo iz stanja vrh + mempool */
    for (int i = 0; i < NPARCELS; i++) {
        const Parcel *p = &g_mem_state.p[i];
        char c = '0';
        if (p->owned) c = !memcmp(p->owner, g_id.pk, 32) ? '2' : !memcmp(p->owner, g_key_b.pk, 32) ? '3' : '1';
        body[n++] = c;
    }
    n += snprintf(body + n, sizeof body - n, "\",\"blocks\":[");
    int shown = 0;
    for (int i = g_tip; i > 0 && shown < 12; i = g_blocks[i].parent, shown++) {
        char lab[96];
        miner_label(g_blocks[i].miner, lab, sizeof lab);
        n += snprintf(body + n, sizeof body - n, "%s{\"height\":%llu,\"miner\":\"%s\",\"me\":%s,\"reward\":%d}",
                      shown ? "," : "", (unsigned long long)g_blocks[i].height, lab,
                      !memcmp(g_blocks[i].miner, g_id.pk, 32) ? "true" : "false", g_blocks[i].reward);
    }
    n += snprintf(body + n, sizeof body - n, "]}");
    http_reply(h, "application/json", body, n);
}

static void http_request(HttpConn *h)
{
    char path[512] = "/";
    sscanf(h->req, "GET %511s", path);
    char *q = strchr(path, '?');
    if (q) *q++ = 0;
    if (!strcmp(path, "/state")) { http_state(h); return; }
    if (!strcmp(path, "/paint")) {
        char ps[16], cs[16], body[256];
        uint8_t rgb[3];
        const char *msg;
        if (!q || query_get(q, "p", ps, sizeof ps) || query_get(q, "c", cs, sizeof cs)) msg = "nedostaje parcela ili boja";
        else if (parse_parcel(ps) < 0) msg = "losa parcela";
        else if (parse_color(cs, rgb)) msg = "losa boja";
        else { msg = paint_parcel(parse_parcel(ps), rgb); logmsg("%s", msg); }
        int n = snprintf(body, sizeof body, "{\"msg\":\"%s\"}", msg);
        http_reply(h, "application/json", body, n);
        return;
    }
    http_reply(h, "text/html; charset=utf-8", UI_HTML, sizeof UI_HTML - 1);
}

static void http_fds(fd_set *rd, fd_set *wr, sock_t *maxfd, int *any)
{
    if (g_http_listen == BAD_SOCK) return;
    FD_SET(g_http_listen, rd);
    if (g_http_listen > *maxfd) *maxfd = g_http_listen;
    *any = 1;
    for (int i = 0; i < MAX_HTTP; i++) {
        if (!g_http[i].used) continue;
        if (g_http[i].out) FD_SET(g_http[i].s, wr); else FD_SET(g_http[i].s, rd);
        if (g_http[i].s > *maxfd) *maxfd = g_http[i].s;
    }
}

static void http_close(HttpConn *h) { sock_close(h->s); free(h->out); memset(h, 0, sizeof *h); }

static void http_io(fd_set *rd, fd_set *wr)
{
    if (g_http_listen == BAD_SOCK) return;
    if (FD_ISSET(g_http_listen, rd)) {
        sock_t s = accept(g_http_listen, NULL, NULL);
        if (s != BAD_SOCK) {
            int slot = -1;
            for (int i = 0; i < MAX_HTTP; i++) if (!g_http[i].used) { slot = i; break; }
            if (slot < 0) sock_close(s);
            else {
                set_nonblock(s);
                memset(&g_http[slot], 0, sizeof g_http[slot]);
                g_http[slot].used = 1; g_http[slot].s = s; g_http[slot].start = now_ms();
            }
        }
    }
    for (int i = 0; i < MAX_HTTP; i++) {
        HttpConn *h = &g_http[i];
        if (!h->used) continue;
        if (now_ms() - h->start > 5000) { http_close(h); continue; }
        if (!h->out && FD_ISSET(h->s, rd)) {
            int n = (int)recv(h->s, h->req + h->rlen, (int)(sizeof h->req - 1 - h->rlen), 0);
            if (n <= 0) { if (n == 0 || !would_block()) http_close(h); continue; }
            h->rlen += n;
            h->req[h->rlen] = 0;
            if (strstr(h->req, "\r\n\r\n") || h->rlen >= (int)sizeof h->req - 1) http_request(h);
        }
        if (h->out && FD_ISSET(h->s, wr)) {
            int n = (int)send(h->s, h->out + h->osent, (int)(h->olen - h->osent), 0);
            if (n < 0) { if (!would_block()) http_close(h); continue; }
            h->osent += n;
            if (h->osent >= h->olen) http_close(h);
        }
    }
}

/* ======================= config.txt ======================= */

static int add_peers(const char *listin)
{
    char list[1024];
    snprintf(list, sizeof list, "%s", listin);
    for (char *tok = strtok(list, ", \t"); tok; tok = strtok(NULL, ", \t")) {
        Peer p; memset(&p, 0, sizeof p); p.session = -1;
        char full[96];
        if (!strchr(tok, ':')) snprintf(full, sizeof full, "127.0.0.1:%s", tok);   /* samo port = isti racunar */
        else snprintf(full, sizeof full, "%s", tok);
        if (parse_addr(full, &p.addr, p.host, &p.port)) { fprintf(stderr, "losa adresa: %s\n", full); return -1; }
        int dup = 0;
        for (int k = 0; k < g_npeers; k++) if (!memcmp(&g_peers[k].addr, &p.addr, sizeof p.addr)) dup = 1;
        if (!dup && g_npeers < MAX_PEERS) g_peers[g_npeers++] = p;
    }
    return 0;
}

static const char CONFIG_TEMPLATE[] =
    "# Pixel Chain - podesavanja cvora\n"
    "# Linije koje pocinju sa # se ignorisu. Opcije iz komandne linije imaju prednost.\n"
    "\n"
    "# Port na kome tvoj cvor slusa (interfejs u browseru je na port + 1000)\n"
    "port=7001\n"
    "\n"
    "# IP adrese i portovi drugih cvorova, razdvojeni zarezom.\n"
    "# Adresa kolege: kolega pokrene ipconfig i procita IPv4 Address.\n"
    "# Primer: peers=192.168.1.20:7001\n"
    "peers=\n"
    "\n"
    "# Rudarenje: 1 = ukljuceno, 0 = iskljuceno\n"
    "mine=1\n"
    "\n"
    "# Folder za kljuceve i sliku platna\n"
    "data=data\n"
    "\n"
    "# Simulacija laga (ostavi 1)\n"
    "lag=1\n"
    "\n"
    "# Automatsko pronalazenje cvorova na mrezi (UDP broadcast)\n"
    "discover=1\n"
    "\n"
    "# Automatske probne transakcije\n"
    "demo=1\n";

/* Ucita config. Ako fajl ne postoji i create = 1, napravi ga iz sablona. */
static int load_config(const char *path, int create)
{
    FILE *f = fopen(path, "r");
    if (!f) {
        if (!create) { fprintf(stderr, "ne mogu da otvorim %s\n", path); return -1; }
        f = fopen(path, "w");
        if (f) { fputs(CONFIG_TEMPLATE, f); fclose(f); }
        printf("Napravljen %s - upisi u njega IP adresu kolege (peers=...).\n", path);
        f = fopen(path, "r");
        if (!f) return 0;
    }
    char line[1100];
    while (fgets(line, sizeof line, f)) {
        line[strcspn(line, "\r\n")] = 0;
        char *p = line;
        while (*p == ' ' || *p == '\t') p++;
        if (*p == '#' || *p == 0) continue;
        char *eq = strchr(p, '=');
        if (!eq) continue;
        *eq = 0;
        char *key = p, *val = eq + 1;
        for (char *e = key + strlen(key); e > key && (e[-1] == ' ' || e[-1] == '\t'); ) *--e = 0;
        while (*val == ' ' || *val == '\t') val++;
        for (char *e = val + strlen(val); e > val && (e[-1] == ' ' || e[-1] == '\t'); ) *--e = 0;
        if (!strcmp(key, "port") && *val) snprintf(g_listen_addr, sizeof g_listen_addr, "0.0.0.0:%s", val);
        else if (!strcmp(key, "listen") && *val) snprintf(g_listen_addr, sizeof g_listen_addr, "%s", val);
        else if (!strcmp(key, "peers")) { if (*val && add_peers(val)) { fclose(f); return -1; } }
        else if (!strcmp(key, "mine")) g_mining = atoi(val) != 0;
        else if (!strcmp(key, "data") && *val) snprintf(g_data_dir, sizeof g_data_dir, "%s", val);
        else if (!strcmp(key, "lag")) g_lag = atoi(val) != 0;
        else if (!strcmp(key, "lag_seed")) g_lag_seed = strtoull(val, NULL, 10);
        else if (!strcmp(key, "discover")) g_discover = atoi(val) != 0;
        else if (!strcmp(key, "demo")) g_demo = atoi(val) != 0;
        else printf("config: nepoznato podesavanje '%s' (preskacem)\n", key);
    }
    fclose(f);
    return 0;
}

/* Moja IP adresa u lokalnoj mrezi (UDP "connect" ne salje nista, samo izabere interfejs) */
static void find_my_ip(void)
{
    sock_t s = socket(AF_INET, SOCK_DGRAM, 0);
    if (s == BAD_SOCK) return;
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons(80);
    inet_pton(AF_INET, "8.8.8.8", &a.sin_addr);
    if (connect(s, (struct sockaddr *)&a, sizeof a) == 0) {
        struct sockaddr_in me;
        socklen_t l = sizeof me;
        if (getsockname(s, (struct sockaddr *)&me, &l) == 0) inet_ntop(AF_INET, &me.sin_addr, g_my_ip, sizeof g_my_ip);
    }
    sock_close(s);
}

/* ======================= main ======================= */

int main(int argc, char **argv)
{
#ifdef _WIN32
    SetConsoleOutputCP(CP_UTF8);
    HANDLE ho = GetStdHandle(STD_OUTPUT_HANDLE);
    DWORD mode;
    if (GetConsoleMode(ho, &mode)) SetConsoleMode(ho, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    WSADATA wsa;
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) { fprintf(stderr, "Winsock nije pokrenut\n"); return 1; }
#else
    signal(SIGPIPE, SIG_IGN);
#endif
    if (sodium_init() < 0) { fprintf(stderr, "libsodium nije pokrenut\n"); return 1; }

    int vectors = 0, no_screen = 0;
    long run_secs = -1, mine_secs = -1;
    for (int i = 0; i < MAX_PEERS; i++) g_peers[i].session = -1;

    /* prvo config.txt (ili --config FAJL), pa opcije iz komandne linije preko njega */
    const char *cfg_path = "config.txt";
    int cfg_given = 0, use_cfg = 1;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--config") && i + 1 < argc) { cfg_path = argv[i + 1]; cfg_given = 1; }
        if (!strcmp(argv[i], "--no-config") || !strcmp(argv[i], "--vectors") || !strcmp(argv[i], "--no-screen")) use_cfg = cfg_given;
    }
    if (use_cfg && load_config(cfg_path, !cfg_given) != 0) return 1;

    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--listen") && i + 1 < argc) snprintf(g_listen_addr, sizeof g_listen_addr, "%s", argv[++i]);
        else if (!strcmp(argv[i], "--port") && i + 1 < argc) snprintf(g_listen_addr, sizeof g_listen_addr, "0.0.0.0:%s", argv[++i]);
        else if (!strcmp(argv[i], "--peers") && i + 1 < argc) { if (add_peers(argv[++i])) return 1; }
        else if (!strcmp(argv[i], "--config") && i + 1 < argc) i++;
        else if (!strcmp(argv[i], "--no-config")) {}
        else if (!strcmp(argv[i], "--data") && i + 1 < argc) snprintf(g_data_dir, sizeof g_data_dir, "%s", argv[++i]);
        else if (!strcmp(argv[i], "--mine")) g_mining = 1;
        else if (!strcmp(argv[i], "--no-lag")) g_lag = 0;
        else if (!strcmp(argv[i], "--lag-seed") && i + 1 < argc) g_lag_seed = strtoull(argv[++i], NULL, 10);
        else if (!strcmp(argv[i], "--no-demo")) g_demo = 0;
        else if (!strcmp(argv[i], "--no-discover")) g_discover = 0;
        else if (!strcmp(argv[i], "--discover-port") && i + 1 < argc) g_discover_port = atoi(argv[++i]);
        else if (!strcmp(argv[i], "--vectors")) vectors = 1;
        else if (!strcmp(argv[i], "--no-screen")) { no_screen = 1; g_echo_log = 1; }           /* za testiranje */
        else if (!strcmp(argv[i], "--run-secs") && i + 1 < argc) run_secs = atol(argv[++i]);
        else if (!strcmp(argv[i], "--mine-secs") && i + 1 < argc) mine_secs = atol(argv[++i]);
        else { fprintf(stderr, "nepoznata opcija: %s\n", argv[i]); return 1; }
    }

    if (self_test() != 0) return 1;
    if (!no_screen && !vectors) console_init();
    if (vectors) { print_vectors(); return 0; }

    MKDIR(g_data_dir);
    find_my_ip();
    key_load_or_create(&g_id, "identity.seed");
    key_load_or_create(&g_key_b, "identity_b.seed");
    chain_init();
    memcpy(g_genesis, g_blocks[0].hash, 32);
    mempool_rebuild(NULL, 0);
    if (listen_on(g_listen_addr) != 0) { fprintf(stderr, "ne mogu da slusam na %s (port zauzet?)\n", g_listen_addr); return 1; }
    {   const char *c = strrchr(g_listen_addr, ':'); if (c) g_tcp_port = atoi(c + 1); }
    if (g_discover) {
        if (discover_open() == 0) logmsg("otkrivanje: objavljujem i slusam na UDP portu %d", g_discover_port);
        else logmsg("otkrivanje: UDP port %d nije dostupan", g_discover_port);
    }
    logmsg("slusam na %s, cvorova u listi: %d", g_listen_addr, g_npeers);
    if (http_open(g_tcp_port + 1000) == 0) logmsg("interfejs: otvori http://localhost:%d u browseru", g_http_port);
    else logmsg("interfejs: port %d je zauzet", g_tcp_port + 1000);

    uint64_t start = now_ms(), next_tick = start, next_draw = start, next_demo = start + 5000;
    uint64_t last_tip_gen = (uint64_t)-1;
    while (run_secs < 0 || now_ms() - start < (uint64_t)run_secs * 1000) {
        net_poll(g_mining ? 0 : 20);
        if (g_mining && mine_secs >= 0 && now_ms() - start > (uint64_t)mine_secs * 1000) { g_mining = 0; g_demo = 0; }
        if (g_mining) mine_slice();
        uint64_t now = now_ms();
        if (now >= next_tick) { tick(); if (g_discover) discover_announce(); next_tick = now + TICK_MS; }
        if (g_demo && now >= next_demo) { demo_tx(); next_demo = now + 4000 + randombytes_uniform(4000); }
        if (g_tip_gen != last_tip_gen) { save_bmp(&g_tip_state); last_tip_gen = g_tip_gen; }
        if (!no_screen && now >= next_draw) { draw_screen(); next_draw = now + 1000; }
        if (!no_screen) console_poll();
        if (g_quit) break;
    }
    /* za testove: ispisi vrh i hes stanja */
    char th[65], sh[65]; uint8_t s[32];
    state_hash(&g_tip_state, s); to_hex(sh, s, 32); to_hex(th, g_blocks[g_tip].hash, 32);
    printf("KRAJ visina=%llu vrh=%s stanje=%s blokova=%d\n", (unsigned long long)g_blocks[g_tip].height, th, sh, g_nblocks);
    return 0;
}