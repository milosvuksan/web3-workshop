# Pixel Chain — grupna specifikacija

Dva pravila važe za ceo dokument:

1. **Bajtovi moraju tačno da se poklapaju.** Za svaki heš i potpis zapisan je tačan niz bajtova.
2. **Specifikacija ima prednost.** Kad se dva čvora ne slažu, prvo se ispravlja ovaj dokument, pa tek onda kod.

> **Status:** sekcije 2–11 preuzete su iz `PROTOTYPE_DEFAULTS.md` (prototip u Rust-u). To su **privremene** odluke dok ih grupa ne potvrdi. Svaka promena pravila konsenzusa (sekcije 2–8) znači novi lanac: novi genesis i ponovo generisane test podatke.

## Pregled odluka

| Odluka | Vrednost |
| --- | --- |
| Potpis | Ed25519 (običan, ne Ed25519ph), stroga provera |
| Heš | SHA-256 |
| Zapis bajtova kao tekst | hex, mala slova, tačna dužina, bez `0x` |
| Brojevi u bajtovima | neoznačeni, fiksne širine, big-endian |
| Platno | 32×32 (1024 parcele), (0,0) gore levo, red po red |
| Težina | 23 bita, fiksna |
| Parcela po bloku | 1 |
| Zauzeta parcela | množenje: `(kandidat · (time mod 1024)) mod 1024`, najviše 1024 kandidata ukupno |
| Fork | najviše rada; pri izjednačenju manji heš vrha |
| Zaštita od ponavljanja | brojač po parceli, počinje od 0 |
| Mreža | TCP, svaki čvor sa svakim, jedan JSON objekat po redu |
| Lag | uključen podrazumevano: 10% gubitka, kašnjenje 20–250 ms |

## 1. Kriptografija i zapis

- Potpis: Ed25519 (RFC 8032), običan, ne Ed25519ph. Javni ključ 32 bajta, seed 32 bajta, potpis 64 bajta. Ključevi moraju biti ispravni i ne „slabi“ (weak), a potpis se proverava strogo.
- Heš: SHA-256, 32 bajta.
- Na granici sa tekstom (JSON, prikaz) javni ključevi, heševi, boje i potpisi su hex **mala slova** tačne dužine, bez `0x`.
- Svi celi brojevi u bajtovima su neoznačeni, fiksne širine, **big-endian**: `u16` = 2 bajta, `u64` = 8 bajtova.
- JSON služi samo za prenos. **Nikad** se ne hešuje i ne potpisuje.

## 2. Transakcija

Dve vrste: **bojenje** (oznaka 1) i **prenos** (oznaka 2). Obe potpisuje trenutni vlasnik parcele.

**Poruka koja se potpisuje**, ovim redom:

| # | Polje | Bajtova | Vrednost |
| --- | --- | --- | --- |
| 1 | Domen | 12 | ASCII `PIXEL-TX-v1` + jedan nulti bajt |
| 2 | Oznaka akcije | 1 | bojenje = 1, prenos = 2 |
| 3 | Parcela | 2 | `u16`, 0–1023 |
| 4 | Brojač | 8 | `u64`, sledeći brojač te parcele |
| 5 | Pošiljalac | 32 | Ed25519 javni ključ |
| 6 | Sadržaj | 3 ili 32 | bojenje: R, G, B; prenos: javni ključ primaoca |

Poruka ima 58 bajtova za bojenje i 87 za prenos.

**Kanonska potpisana transakcija** = poruka + potpis (64 bajta): **122 bajta** za bojenje, **151** za prenos.

**ID transakcije** = `SHA-256("PIXEL-ID-v1" + nulti bajt + kanonska potpisana transakcija)`.

**JSON na mreži:**

```json
{"parcel":181,"counter":0,"sender":"<64 hex>","action":{"type":"paint","color":"ff8000"},"signature":"<128 hex>"}
```

Kod prenosa `action` je `{"type":"transfer","recipient":"<64 hex>"}`. Dodatna polja nisu dozvoljena. Čitači JSON-a moraju tačno da sačuvaju svih 64 bita celih brojeva; JavaScript `Number` nije dovoljan za brojače, vremena, visine i nonce-ove.

## 3. Stanje i zaštita od ponavljanja

- Svaka parcela na početku nema vlasnika, crna je (`000000`) i ima brojač 0.
- Nagrada rudaru postavlja vlasnika, ali **ne menja boju**.
- Prenos menja vlasnika, a čuva boju i brojač. Prenos samom sebi je dozvoljen i povećava brojač.
- Transakcija je ispravna samo ako je njen brojač jednak **sledećem brojaču** parcele: prva transakcija nad parcelom ima brojač 0, sledeća 1 itd.
- Brojač prati parcelu kroz sve prenose, i kad se vrati ranijem vlasniku. To odbija ponovno uključivanje iste transakcije, ponavljanje posle prenosa i dvostruku upotrebu istog brojača.
- Transakcija sa brojačem `u64::MAX` se odbija; ta parcela je zamrznuta.
- Pri reorganizaciji lanca vraća se celo stanje pobedničke grane, uključujući brojače.

**Provera transakcije, ovim redom:**

1. Parcela je u opsegu (dužine polja, hex i boja su već strogo dekodirani).
2. Ključ pošiljaoca je ispravan i ne slab, a potpis prolazi strogu proveru.
3. Kod prenosa je ključ primaoca ispravan i ne slab.
4. Pošiljalac poseduje parcelu u privremenom stanju na tom mestu u bloku.
5. Brojač je jednak sledećem brojaču parcele, a povećanje ne prelazi `u64`.

Tek tada se akcija primenjuje i brojač povećava.

## 4. Blok

**Commitment transakcija** = `SHA-256` redom nadovezanih parova `u16 dužina ‖ kanonska potpisana transakcija` za svaku transakciju. Za praznu listu je to `SHA-256` praznog niza. Nema Merkle stabla.

**Zaglavlje bloka: 137 bajtova** (heš bloka = `SHA-256(zaglavlje)`), ovim redom:

| # | Polje | Bajtova |
| --- | --- | --- |
| 1 | ASCII `PIXEL-BLOCK-v1` + jedan nulti bajt | 15 |
| 2 | Heš prethodnog bloka | 32 |
| 3 | Visina (`u64`) | 8 |
| 4 | Javni ključ rudara | 32 |
| 5 | Unix vreme u sekundama (`u64`) | 8 |
| 6 | Broj transakcija (`u16`) | 2 |
| 7 | Commitment transakcija | 32 |
| 8 | Nonce (`u64`) | 8 |

- Najviše **256** transakcija po bloku.
- Lista transakcija putuje uz blok, a primalac sam računa commitment.
- Blok nema potpis, a na mreži nema ni posebno polje sa hešom.

**Redosled u bloku:** transakcije se izvršavaju redom kako su navedene, **pre** dodele nagrade. Parcela dobijena kao nagrada ne može se koristiti u istom bloku.

## 5. Genesis blok

Prethodni heš su nule, visina 0, rudar 32 nulta bajta, vreme 0, nonce 0, prazna lista transakcija. Samo taj tačan blok je genesis.

- Izuzet je od proof of work-a i od provere ključa rudara.
- Ne dodeljuje parcelu i doprinosi nula rada.
- **Heš genesis bloka:** `56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce`

## 6. Težina, parcele i zauzetost

**Težina:** 23 vodeća nulta bita, fiksno. Bitovi se broje od prvog bajta heša, od najvišeg bita.

**Platno:** 32×32, parcele 0–1023, (0,0) je gornji levi ugao, red po red: `x = p mod 32`, `y = p / 32`. Boje su RGB, bez providnosti.

**Prvi kandidat:** `p₀ = (256 · hash[30] + hash[31]) mod 1024`, gde je `hash[0]` prvi bajt heša bloka. Parcela 0 je ispravan kandidat.

**Ako je kandidat zauzet:** `pᵢ₊₁ = (pᵢ · (time mod 1024)) mod 1024`. To je matematički isto kao dogovoreni proizvod, samo bez prekoračenja. Pokušava se **najviše 1024 kandidata ukupno**, računajući i prvi. Ako se iscrpe pokušaji ili je platno puno, nema nagrade, a blok ostaje ispravan.

Napomena: pošto je 1024 = 2¹⁰, množenje može da upadne u kratak ciklus (npr. sa parnim vremenom posle najviše 10 koraka ostaje na parceli 0). To je **namerno zadržano**; ne zamenjuje se sabiranjem.

**Brzine (za proveru težine):**

| Član | Jezik | Heševa u sekundi |
| --- | --- | --- |
| ____ | C | ~727.000 |
| ____ | C++ | ____ |
| ____ | Rust | ____ |
| ____ | Go | ____ |

## 7. Pravila ispravnosti bloka

Za svaki blok osim genesis-a, ovim redom:

1. Broj transakcija ≤ 256.
2. Prethodni blok je poznat i heš se poklapa.
3. Visina = visina prethodnog + 1, bez prekoračenja.
4. Ključ rudara je ispravan i ne slab.
5. Vreme ≥ vreme prethodnog bloka (jednako je dozvoljeno).
6. Vreme ≤ vreme primaoca + 120 sekundi.
7. Heš ima bar 23 vodeća nulta bita.
8. Svaka transakcija redom, nad stanjem posle prethodnih (sekcija 3).
9. Nagrada (sekcija 6).

- Pre izvršavanja transakcija stanje se kopira. Bilo koja greška odbija ceo blok bez izmene stanja.
- Blokovi iz budućnosti se odlažu i ponovo pokušavaju kad postanu prihvatljivi, ne odbacuju se trajno.
- Blokovi sa nepoznatim roditeljem čekaju (pending); to ne znači da su neispravni.

## 8. Forkovi

- Svaki blok osim genesis-a doprinosi tačno `2^23` jedinica rada, bez obzira na dodatne nulte bitove.
- Rad lanca = visina × `2^23`, čuva se kao `u128`.
- Pobeđuje veći rad. Pri izjednačenju pobeđuje vrh sa **manjim heš-om** (32 bajta poređena redom).
- Čvor čuva sve ispravne sporedne grane u memoriji.
- Pri reorganizaciji se odvojene transakcije razmatraju zajedno sa postojećim mempool-om, po redosledu zavisnosti. Ostaju samo one koje su ispravne nad novim stanjem; već potvrđene ili konfliktne se uklanjaju.

## 9. Mreža i poruke

- TCP, UTF-8 JSON objekti, **jedan objekat po redu** (`\n`). Redosled polja i razmaci ne utiču na konsenzus.
- Najviše **262.144 bajta** pre `\n`. Nepotpuni redovi se baferuju do te granice.
- Neispravan UTF-8, loš JSON, loša polja fiksne širine, dodatna polja, nepoznat tip poruke ili predugačak red **zatvaraju tu TCP sesiju**.
- Transakcije i blokovi koji nisu ispravni po konsenzusu se ignorišu; neispravni blokovi se beleže u log.
- Sesija bez ijednog bajta 30 sekundi se zatvara.

**Vrste poruka** (polje `type`):

| `type` | Ostala polja | Svrha / odgovor |
| --- | --- | --- |
| `hello` | `version:1`, `genesis:<heš>`, `identity:<javni ključ>` | identifikacija lanca i verzije; obavezan pre svih drugih poruka |
| `get_tip` | — | odgovor: `tip` |
| `tip` | `hash:<heš>`, `height:<u64>` | otkrivanje nepoznatog vrha; visina je samo savet, nikad se ne veruje kao rad |
| `get_block` | `hash:<heš>` | odgovor `block` ako je poznat; ako nije, nema odgovora |
| `block` | `block:<objekat>` | polja: `previous`, `height`, `miner`, `timestamp`, `nonce`, `transactions` |
| `transaction` | `transaction:<objekat>` | potpisano bojenje ili prenos (sekcija 2) |

**Hello:**

- Drugačija verzija ili genesis, loš ili slab identitet, ili sopstveni identitet odbijaju sesiju.
- Identitet služi samo za otkrivanje, nije prijava. Ispravnost blokova i transakcija je jedino što se računa.
- `hello` se ponavlja na svake 2 sekunde, i posle prihvatanja, da izgubljeni handshake ne zaglavi drugu stranu.
- Prvi prihvaćen `hello` izaziva naš `hello` i `tip`.
- Svaka spremna sesija razmenjuje `get_tip` i `tip` na svake 2 sekunde.

**Adrese i veze:**

- Adrese se zadaju eksplicitno kroz `--peers` (ponovljeno ili razdvojeno zarezima). Svaki čvor navodi sve ostale; duplikati se spajaju.
- Istovremene dolazne i odlazne sesije se obe zadržavaju (obično dve TCP sesije po paru). Deduplikacija blokova i transakcija sprečava petlje.
- Opciono UDP otkrivanje (`--discover`, port 7000) opisano je u `NETWORK.md`; nije deo konsenzusa. Bez njega se koriste eksplicitni `--peers`.

**Prosleđivanje i sustizanje:**

- Novi ispravni blokovi (i sa sporednih grana) i novoprihvaćene transakcije šalju se **jednom** svim spremnim sesijama, uključujući i onu od koje su stigli. Već poznati se ne šalju ponovo.
- Periodična razmena `tip` poruka otkriva propuštene blokove.
- Nedostajući preci se traže unazad, pa se proveravaju unapred.
- Nedostajući heševi se ponovo traže na svake 2 sekunde od svih spremnih čvorova, do 4 heša po krugu, rotirajući redom. Najviše 1024 tražena heša; kad se memorija za zahteve napuni, izbacuje se najstariji zahtev. Poznati blokovi (u lancu ili na čekanju) se ne traže ponovo.
- Blok sa nepoznatim roditeljem mora imati 23 nulta bita pre čuvanja na čekanju. Čuva se najviše 1024 bloka na čekanju; pri popunjavanju izbacuje se blok sa najvećom visinom, da preci imaju prednost.
- Kad stigne blok bez poznatog roditelja, odmah se traži roditelj od istog izvora.
- Do 4 transakcije iz mempool-a se ponovo najavljuju po krugu, rotirajući redom.

## 10. Priključenje, pad i restart

- Lanac se čuva samo u memoriji. Posle restarta čvor ga ponovo preuzme od preživelih čvorova kroz `hello`, `tip` i `get_block`.
- Ako se ugase svi čvorovi, lanac se gubi. Za test oporavka bar jedan čvor mora ostati upaljen.
- Ponovno povezivanje posle 2 sekunde; timeout za povezivanje i slanje 5 sekundi; ponovni zahtevi na 2 sekunde.

## 11. Simulacija laga

- Uključena podrazumevano; isključuje se samo sa `--no-lag`.
- Primenjuje se nezavisno na svaku odlaznu poruku po TCP sesiji, uključujući `hello`, zahteve, odgovore i prosleđivanja.
- **10%** poruka se gubi. Preostale kasne nasumično, ravnomerno **20–250 ms**, i čuvaju redosled unutar sesije.
- Nasumičnost laga: xorshift64 sa semenom `--lag-seed` (podrazumevano 1), pomešanim sa rednim brojem veze. Nikad se ne koristi za ključeve.

## 12. Lokalna podešavanja (nisu konsenzus)

Ovo je prototip u Rust-u; drugi čvorovi ne moraju da ga prate, ali je korisno zbog doslednosti.

| Podešavanje | Podrazumevano |
| --- | --- |
| Adresa za slušanje | `0.0.0.0:7001`; port se čuva u settings.json, `--port` ili `--listen` ga privremeno menjaju |
| Folder sa podacima | `data/node`; poseban folder za svaki pokrenut proces |
| Identitet | `identity.seed` u folderu sa podacima, pravi se automatski; `--key PATH` učitava postojeći |
| Seed fajl | 64 hex znaka mala slova, najviše jedan `\n` na kraju; privatni bajtovi se nikad ne ispisuju |
| Dijagnostika | `status.json`, `state.json` i `canvas.ppm` (32×32, P6), osvežavaju se na 2 sekunde |
| Heš stanja (dijagnostika) | SHA-256 parcela red po red: vlasnik postoji `u8`, bajtovi vlasnika (nule ako ga nema), RGB 3 bajta, sledeći brojač `u64`; nije polje konsenzusa |
| Rudarenje | isključeno dok se ne pokrene `--mine` ili `mining on`; jedna radna nit; fiksnih 23 bita |
| Mempool | 4096 transakcija; najviše 256 po bloku |
| Blokovi na čekanju | najviše 1024 bloka i 1024 tražena heša |
| Najviše sesija | 128 dolaznih |

## 13. Test podaci

**Provera alata:**

| Test | Ulaz | Očekivano |
| --- | --- | --- |
| SHA-256 | `abc` | `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` |
| Ed25519 javni ključ (RFC 8032, test 1) | seed `9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60` | `d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a` |
| Genesis heš | zaglavlje iz sekcije 5 (137 bajtova) | `56ecd78b0620f10bcb47263b61ca4e5fbbfeaea4a6860ed24cc2b19d36f2e1ce` |

**Test podaci prototipa:** u fajlu `tests/fixtures/prototype-v1.json` (generiše ih `examples/generate_vectors.rs`). Pokrivaju ključeve, bojenje, prenos, genesis, blok 1 sa težinom, parcelu bloka 1 (**181**), zauzetu parcelu (sledeći kandidat je 0), neispravnog potpisnika, kao i forkove i neispravne blokove. Seed-ovi u njima su javni i nikad se ne koriste za prave identitete.

Nezavisna provera u Pythonu: `scripts/verify-vectors.py`.

## Šta grupa još treba da uradi

- [ ] Prihvatiti ili zameniti svaku privremenu odluku iz sekcija 2–11
- [ ] Svaki član proveri test podatke iz `prototype-v1.json` u svom jeziku
- [ ] Upisati brzine svih čvorova i proveriti težinu
- [ ] Pokrenuti celu mrežu na LAN-u i scenario sa mentorom
