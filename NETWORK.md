# Pixel Chain — mreža: uputstvo za implementaciju

Ovaj dokument opisuje kako čvorovi razgovaraju preko portova, toliko detaljno da ga može implementirati bilo ko, u bilo kom jeziku. Pravila konsenzusa (bajtovi transakcija i blokova, provere, forkovi) su u `SPEC.md`; ovde je samo mreža.

Referentna implementacija je `node.c` (C + libsodium). Tvoj čvor je ispravan kada se poveže sa njim, sa uključenim lagom, i oba čvora prikažu isti **VRH** i isto **STANJE**.

## 1. Pregled

| Šta | Protokol | Port (podrazumevano) | Deo specifikacije? |
| --- | --- | --- | --- |
| Blokovi, transakcije, sustizanje | TCP | **7001** (`--port`) | da, `SPEC.md` sekcija 9 |
| Automatsko pronalaženje čvorova | UDP broadcast | **7000** (`--discover-port`) | **ne**, dodatak; vidi sekciju 2 |

Svaki čvor:

1. sluša TCP na svom portu (npr. `0.0.0.0:7001`, da ga vide i drugi računari);
2. otvara TCP vezu ka svakom poznatom čvoru (iz `--peers` ili pronađenom preko UDP-a);
3. po svakoj vezi šalje i prima JSON poruke, jednu po redu;
4. svaku poruku koju šalje propušta kroz simulaciju laga.

Između dva čvora obično postoje **dve** TCP veze (svaki se povezao ka drugom). Obe se zadržavaju; duplikate blokova i transakcija sprečava deduplikacija.

## 2. UDP pronalaženje čvorova (opciono)

Ovo nije u `SPEC.md`. Čvor koji ga ne podržava i dalje radi sa ostalima preko `--peers`. Ako ga grupa prihvati, treba ga upisati u specifikaciju.

**Paket** (jedan UDP datagram, ASCII, bez novog reda na kraju, reči razdvojene jednim razmakom):

```
PIXELCHAIN v1 <genesis heš, 64 hex> <TCP port, decimalno> <identitet, 64 hex>
```

Primer:

```
PIXELCHAIN v1 56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce 7001 8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c
```

**Slanje**, na svake 2 sekunde, isti paket na dve adrese:

- `255.255.255.255:7000`, cela lokalna mreža;
- `127.0.0.1:7000`, čvorovi na istom računaru.

**Socket:**

- UDP, vezan za `0.0.0.0:7000`;
- `SO_BROADCAST` uključen (bez njega slanje na 255.255.255.255 ne radi);
- `SO_REUSEADDR` uključen, a na Linux-u/macOS-u i `SO_REUSEPORT`, da više čvorova na istom računaru može da sluša isti port.

**Prijem.** Paket se ignoriše ako:

- ne počinje sa `PIXELCHAIN v1`, ili nema tačno 5 reči;
- port nije 1–65535;
- genesis nije naš (drugi lanac);
- identitet nije ispravan Ed25519 ključ;
- identitet je naš (to smo mi sami).

Inače je novi čvor na adresi **IP pošiljaoca datagrama** (ne iz sadržaja) + **TCP port iz paketa**. Dodaje se u listu ako u njoj već nema nikoga sa istim identitetom ili istom adresom. Od tada se sa njim postupa isto kao sa čvorom iz `--peers`.

Napomena: neke mreže (fakultetski Wi-Fi, mreže za goste) blokiraju broadcast. Tada se koristi `--peers`.

## 3. TCP: okvir poruka

- Svaka poruka je **jedan JSON objekat u jednom redu**, UTF-8, završen sa `\n` (bajt `0x0A`). Unutar JSON-a nema novih redova.
- Najviše **262.144 bajta** pre `\n`. Nepotpun red se čuva u baferu dok ne stigne ostatak.
- TCP je tok bajtova: jedan `recv` može doneti pola poruke ili tri poruke odjednom. Čitaj u bafer i seci po `\n`.
- Redosled polja i razmaci u JSON-u ne utiču na konsenzus.

**Zatvori sesiju** (samo tu TCP vezu) ako stigne:

- neispravan UTF-8 ili neispravan JSON;
- polje pogrešnog tipa ili dužine (npr. hex od 63 znaka, velika slova, broj sa decimalom ili negativan);
- dodatno ili nedostajuće polje;
- nepoznat `type`;
- red duži od granice.

Blokove i transakcije koji su **dobro formirani, ali neispravni po konsenzusu** (loš potpis, nije vlasnik, nedovoljno rada) samo ignoriši. Za njih se sesija ne zatvara.

Celi brojevi su `u64`: parser mora tačno da sačuva svih 64 bita (JavaScript `Number` nije dovoljan).

## 4. Poruke

Svaka poruka ima polje `type`. Za svaki tip polja moraju biti **tačno** ova.

### `hello`

```json
{"type":"hello","version":1,"genesis":"56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce","identity":"<64 hex>"}
```

- `identity` je javni ključ čvora. U `node.c` je to isti ključ kojim čvor rudari, pa ga drugi čvorovi koriste za prikaz ko je izrudario blok.
- Primalac **zatvara sesiju** ako je `version` ≠ 1, `genesis` nije naš, `identity` nije ispravan ključ ili je `identity` naš.
- Dok ne stigne ispravan `hello`, sve druge poruke se ignorišu (ne zatvara se sesija).

### `get_tip` i `tip`

```json
{"type":"get_tip"}
{"type":"tip","hash":"<64 hex>","height":42}
```

- Na `get_tip` se odgovara porukom `tip` sa trenutnim vrhom.
- Kad stigne `tip` sa hešom koji ne znamo: dodaj ga u **tražene**, i odmah pošalji `get_block` tom čvoru.
- `height` je samo savet. Nikad se ne koristi za odluku koji lanac je bolji.

### `get_block`

```json
{"type":"get_block","hash":"<64 hex>"}
```

Ako znamo taj blok (u lancu, i na sporednoj grani), odgovori porukom `block`. Ako ne znamo, ne odgovaraj ništa.

### `block`

```json
{"type":"block","block":{"previous":"<64 hex>","height":1,"miner":"<64 hex>","timestamp":1763100000,"nonce":3926897,"transactions":[ ... ]}}
```

- Nema polja sa hešom: primalac sam računa commitment transakcija i heš zaglavlja (`SHA-256` 137 bajtova, `SPEC.md` sekcija 4).
- `transactions` je niz transakcija u istom obliku kao u poruci `transaction` (polje `transaction` iznutra).

Genesis kao poruka (dobro za prvu proveru parsera):

```json
{"type":"block","block":{"previous":"0000000000000000000000000000000000000000000000000000000000000000","height":0,"miner":"0000000000000000000000000000000000000000000000000000000000000000","timestamp":0,"nonce":0,"transactions":[]}}
```

### `transaction`

Bojenje (ključ A iz test podataka, seed `01…01`, parcela 181, brojač 0, boja `ff8000`):

```json
{"type":"transaction","transaction":{"parcel":181,"counter":0,"sender":"8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c","action":{"type":"paint","color":"ff8000"},"signature":"f6e77abad4b93a31784bfaa4824108e5335f8f027e8df9fbfd20d55a8d0434bbc2017053a5345f0f76947ead36d832728283994276d9ca85380f4225acc3560a"}}
```

Njen ID je `f9b61b328fa0214ae3b25c7f32c124831770317aab1419483d966134a77123cd`.

Prenos te parcele sa A na B (seed `02…02`), brojač 1:

```json
{"parcel":181,"counter":1,"sender":"8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c","action":{"type":"transfer","recipient":"8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394"},"signature":"41b4f2b04ab668cb1505494724569b7e519516709ebd13efecaffd7a3676e47f8f35845fb9d5507b18950d2b77ab2d238214bdc0347a8dcf2f61b3dac3ff3d06"}
```

Ako tvoj kod iz ovog JSON-a napravi kanonske bajtove (`SPEC.md` sekcija 2) i potpis prođe proveru, format transakcija ti je dobar.

## 5. Životni vek veze

```
odlazna veza (mi zovemo)              dolazna veza (neko zove nas)
-----------------------               ----------------------------
connect() ka peer-u                   accept()
   | (timeout 5 s, pa ponovo za 2 s)     |
   v                                     v
 posalji hello  ---------------------->  posalji hello
                <----------------------
 primljen ispravan hello?  -> sesija je SPREMNA
   da: posalji hello + tip (samo prvi put)
```

- Odmah posle povezivanja **obe strane** šalju `hello`.
- Prvi ispravan `hello` čini sesiju **spremnom**. Tada odgovori svojim `hello` i porukom `tip`.
- `hello` se ponavlja **na svake 2 sekunde, zauvek**, i posle spremnosti. Zbog laga se poruke gube, a ponavljanje garantuje da nijedna strana ne ostane zaglavljena.
- Ako se odlazna veza prekine ili ne uspe, pokušaj ponovo posle **2 sekunde**.
- Sesija bez ijednog primljenog bajta **30 sekundi** se zatvara.

## 6. Periodični poslovi (na svake 2 sekunde)

Za svaku sesiju:

1. pošalji `hello`;
2. ako je spremna: pošalji `get_tip` i svoj `tip`.

Globalno:

3. **Traženi blokovi:** uzmi do 4 heša iz liste traženih (kružno, sledeći put sledeća 4) i pošalji `get_block` za svaki **svim** spremnim sesijama. Tako sustizanje ne staje ako čvor koji je imao blok nestane.
4. **Mempool:** uzmi do 4 transakcije iz mempool-a (kružno) i ponovo ih pošalji svim spremnim sesijama. Tako se oporavljaju izgubljene najave.
5. **Blokovi iz budućnosti:** probaj ponovo blokove koji su čekali jer im je `timestamp` bio više od 120 s ispred sata.

## 7. Šta raditi kad stigne blok

```
primi block:
    izracunaj commitment i hes iz polja
    ako vec znam taj hes (u lancu ili na cekanju): kraj
    proveri redom (SPEC.md sekcija 7):
        broj tx <= 256                         inace: neispravan
        roditelj poznat?                       ne: NA CEKANJE
        visina = roditelj + 1                  inace: neispravan
        kljuc rudara ispravan                  inace: neispravan
        timestamp >= timestamp roditelja       inace: neispravan
        timestamp <= sada + 120 s              inace: IZ BUDUCNOSTI (cuvaj, probaj kasnije)
        hes ima >= 23 nulta bita               inace: neispravan
        sve transakcije redom nad stanjem roditelja
        nagrada
    ISPRAVAN:
        dodaj u lanac (i ako je sporedna grana)
        ako je novi lanac bolji: novi vrh, preracunaj stanje, sredi mempool
        posalji blok JEDNOM svim spremnim sesijama (ukljucujuci i onu od koje je stigao)
        proveri blokove na cekanju: ako im je roditelj sada poznat, obradi ih istim postupkom
    NA CEKANJE:
        ako hes nema >= 23 nulta bita: odbaci
        sacuvaj blok (najvise 1024)
        ako roditelj nije poznat (ni u lancu ni na cekanju):
            dodaj heš roditelja u trazene
            odmah posalji get_block za roditelja istoj sesiji
    NEISPRAVAN:
        upisi u log, ne prosledjuj
```

Sustizanje posle kasnog uključenja ili restarta ide samo od sebe: stigne `tip` sa nepoznatim hešom, pa `get_block`. Blok ima nepoznatog roditelja, pa `get_block` za roditelja, i tako unazad do bloka koji imamo. Onda se blokovi na čekanju obrađuju unapred.

Poznat blok je u lancu ili na čekanju i ne traži se ponovo. Heš se skida iz traženih čim blok postane poznat. Na čekanju se čuva do 1024 bloka; pri popunjavanju izbacuje se blok sa najvećom visinom. Traži se do 1024 heša; novi zahtev pri popunjavanju izbacuje najstariji. Duplikat zahteva ne menja njegovu starost. Detalji i provere: [CATCHUP.md](CATCHUP.md).

## 8. Šta raditi kad stigne transakcija

```
primi transaction:
    ako je vec u mempool-u (isti ID): kraj
    proveri nad stanjem (vrh + sve iz mempool-a, redom), SPEC.md sekcija 3
    ispravna: dodaj na kraj mempool-a i posalji JEDNOM svim spremnim sesijama
    neispravna: ignorisi (mozda postane ispravna kasnije; posiljalac ce je ponovo najaviti)
```

Pri promeni vrha napravi mempool ponovo. Uzmi transakcije sa napuštene grane (ako je bilo reorganizacije), pa postojeći mempool, i zadrži samo one koje su redom ispravne nad novim stanjem.

## 9. Simulacija laga

Uključena podrazumevano; isključuje se sa `--no-lag`. Primenjuje se na **svaku odlaznu TCP poruku**, uključujući `hello`, zahteve i odgovore.

```
posalji(sesija, poruka):
    ako lag:
        ako rand() % 100 < 10: kraj                        # 10% se izgubi
        kada = sada_ms + 20 + rand() % 231                 # 20..250 ms
    inace:
        kada = sada_ms
    kada = max(kada, sesija.poslednje_kada)                # redosled u sesiji ostaje
    sesija.poslednje_kada = kada
    ako u redu sesije vec ima 128 poruka: zatvori sesiju
    stavi (kada, poruka) u red sesije

svakih nekoliko ms:
    za svaku sesiju posalji sve poruke iz reda kojima je "kada" <= sada_ms
```

Nasumičnost: `xorshift64` sa semenom `--lag-seed` (podrazumevano 1), pomešanim sa rednim brojem veze, posebno za svaku sesiju:

```
x ^= x << 13;  x ^= x >> 7;  x ^= x << 17;  return x
```

Lag nikad ne koristi isti generator kao ključevi.

## 10. Kako organizovati program

**Jedna nit (tako radi `node.c`):**

```
petlja:
    select/poll na: TCP listen socket, UDP socket, sve sesije  (timeout 0 ms ako rudarim, inace 20 ms)
    prihvati nove veze, procitaj sta je stiglo, obradi cele redove
    posalji poruke iz redova ciji je trenutak dosao
    ako rudarim: probaj 16.384 nonce-a
    na 2 s: periodicni poslovi + UDP objava
    na 1 s: osvezi ekran
```

Kad se promeni vrh ili mempool, napravi novog kandidata za rudarenje. Stari nonce-ovi više ne važe.

**Više niti** je takođe u redu. Jedna nit rudari i povremeno proverava „da li se promenio vrh“, a ostale rade mrežu. Stanje lanca menja samo jedna nit ili ga čuva brava.

Šta je pri ruci po jeziku:

| Jezik | TCP i UDP | JSON |
| --- | --- | --- |
| Go | `net.Listen`, `net.Dial`, `net.ListenPacket` + gorutina po vezi | `encoding/json` (pazi na `u64`: koristi `uint64`, ne `float64`) |
| Rust | `tokio::net::{TcpListener, TcpStream, UdpSocket}` | `serde_json` sa `deny_unknown_fields` |
| C++ | Asio, ili POSIX/Winsock socketi kao u `node.c` | `nlohmann/json` |
| C | Winsock (`-lws2_32`) / POSIX socketi + `select` | ručni parser, kao u `node.c` |

## 11. Prikaz: ko je izrudario blok

Polje `miner` u bloku je javni ključ rudara. Da bi pored bloka pisalo ime ili adresa:

- ako je `miner` naš ključ, piše „JA“;
- ako je jednak `identity` iz `hello` neke spremne sesije, piše adresa te sesije;
- inače piše početak ključa (prvih 8 hex znakova).

Tako oba čvora prikazuju iste blokove, a svaki svoje označava sa „JA“.

## 12. Provera sa `node.c`

1. **Parser:** pošalji genesis poruku iz sekcije 4. Tvoj čvor mora izračunati heš `56ecd78b…`.
2. **Transakcija:** učitaj JSON bojenja iz sekcije 4. Potpis mora proći, a ID mora biti `f9b61b32…`.
3. **Veza:** pokreni `node.c` i svoj čvor:
   ```
   node.exe --port 7001 --peers <tvoj IP>:7002 --mine
   ```
   tvoj čvor na portu 7002 sa `--peers <IP>:7001`. Na ekranu `node.c` mora pisati `povezan sa …`.
4. **Slaganje:** pustite oba da rudare sa uključenim lagom nekoliko minuta, pa zaustavite rudarenje. Posle par sekundi oba čvora moraju imati isti vrh i isti heš stanja (dijagnostički heš, `SPEC.md` sekcija 12).
5. **Pad:** ubij jedan čvor, sačekaj, ponovo ga pokreni. Mora da sustigne drugog.

| Šta `node.c` piše | Šta znači |
| --- | --- |
| `veza … zatvorena: neispravna poruka` | tvoj JSON nije tačno po sekciji 4 (polje viška, velika slova u hex-u, pogrešan tip) |
| `ODBIJEN blok …: nedovoljno rada` | tvoj heš bloka se ne poklapa (najčešće pogrešni bajtovi zaglavlja ili commitment) |
| `ODBIJEN blok …: los kljuc ili potpis` | pogrešni bajtovi koji se potpisuju (domen, redosled, big-endian) |
| ništa se ne dešava, nema `povezan sa` | firewall, pogrešan IP/port, ili `hello` sa drugim genesis-om |
