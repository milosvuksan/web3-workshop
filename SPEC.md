# Pixel Chain — grupna specifikacija

Svako polje `____` popunjava grupa. Odluke koje još nisu potvrđene označene su sa **[predlog]**.

Dva pravila važe za ceo dokument:

1. **Bajtovi moraju tačno da se poklapaju.** Za svaki heš i potpis zapisan je tačan niz bajtova: redosled polja, dužina svakog polja u bajtovima i redosled bajtova u brojevima.
2. **Specifikacija ima prednost.** Kad se dva čvora ne slažu, prvo se ispravlja ovaj dokument, pa tek onda kod.

## Pregled odluka

| Odluka | Vrednost | Status |
| --- | --- | --- |
| Potpis | Ed25519 | [predlog] |
| Heš | SHA-256 | [predlog] |
| Zapis bajtova kao tekst | hex, mala slova | [predlog] |
| Mreža | TCP, svaki čvor sa svakim | [predlog] |
| Poruka na mreži | jedna poruka po redu, završena sa `\n` | [predlog] |
| Sadržaj poruke | JSON ili prosti tekst: ____ | otvoreno |
| Platno | 64×32 (2048 parcela): 64 kolone, 32 reda | dogovoreno |
| Parcela iz heša | poslednja 2 bajta heša bloka kao broj (big-endian), p = broj mod 2048 | [predlog] |
| Parcela po bloku | 1 | [predlog] |
| Zauzeta parcela | ponavljanje sa vremenom bloka, vidi sekciju 6 | dogovoreno |
| Težina | 23 bita | dogovoreno |
| Fork | najviše rada; pri izjednačenju leksikografski prvi heš poslednjeg bloka | dogovoreno |

## 1. Kriptografija i zapis

- Potpis: Ed25519
- Heš: SHA-256
- Javni ključ: 32 bajta; zapisuje se kao ____
- Privatni ključ (seed): 32 bajta; gde ga čvor čuva između pokretanja: ____
- Potpis: 64 bajta; zapisuje se kao ____
- Heš: 32 bajta; zapisuje se kao ____
- Mala ili velika slova u hex zapisu: ____
- Brojevi u bajtovima (big-endian ili little-endian): ____

## 2. Transakcija

Dve vrste transakcija: bojenje i prenos. Obe potpisuje trenutni vlasnik parcele.

**Polja transakcije** (redosled u tabeli je redosled bajtova za potpis):

| # | Polje | Bajtova | Format i značenje |
| --- | --- | --- | --- |
| 1 | ____ | ____ | ____ |
| 2 | ____ | ____ | ____ |
| 3 | ____ | ____ | ____ |
| 4 | ____ | ____ | ____ |
| 5 | ____ | ____ | ____ |

Pitanja na koja tabela mora da odgovori:

- Kako se razlikuju bojenje i prenos (polje za tip i njegove vrednosti)?
- Kako se zapisuje boja (koliko bajtova, RGB ili indeks iz palete)?
- Kako se zapisuje parcela (dva bajta 0–2047 ili x i y posebno)?
- Koje polje služi za zaštitu od ponavljanja (sekcija 3)?
- Potpis nije deo bajtova koji se potpisuju.

**Bajtovi za potpis:** ____

**Kako izgleda transakcija u poruci na mreži:** ____

**ID transakcije** (čemu se heš računa, ako se koristi): ____

## 3. Zaštita od ponavljanja

Ista potpisana transakcija poslata dvaput, ili ponovo posle nekog vremena, ne sme drugi put da promeni platno.

- Tehnika koju koristimo (brojač po vlasniku, brojač po parceli, vezivanje za stanje, pamćenje viđenih ili nešto drugo): ____
- Tačno pravilo kad se transakcija odbija kao ponovljena: ____
- Šta važi posle prenosa parcele na novog vlasnika: ____
- Šta važi kad se lanac prebaci na drugi fork: ____

## 4. Blok

**Polja bloka** (redosled u tabeli je redosled bajtova za heš):

| # | Polje | Bajtova | Format i značenje |
| --- | --- | --- | --- |
| 1 | ____ | ____ | ____ |
| 2 | ____ | ____ | ____ |
| 3 | ____ | ____ | ____ |
| 4 | ____ | ____ | ____ |
| 5 | ____ | ____ | ____ |
| 6 | ____ | ____ | ____ |

Blok **mora** da sadrži vreme (unix timestamp) unutar bajtova koji se hešuju, jer ga koristi pravilo za zauzetu parcelu (sekcija 6). Dužina i format: ____ (npr. 8 bajtova, big-endian, sekunde).

Pitanja na koja tabela mora da odgovori:

- Gde je heš prethodnog bloka?
- Ko je rudar (polje koje dobija parcelu)?
- Kako transakcije ulaze u heš: cele redom, ili kao jedan heš liste?
- Koliko najviše transakcija sme u jedan blok?
- Kako se proverava vreme bloka (npr. ne sme biti previše u budućnosti): ____
- Gde je nonce i koliko bajtova ima?

**Bajtovi za heš bloka:** ____

**Kako izgleda blok u poruci na mreži:** ____

## 5. Genesis blok

Genesis je isti na svakom čvoru, bajt po bajt.

| Polje | Vrednost |
| --- | --- |
| ____ | ____ |
| ____ | ____ |
| ____ | ____ |

- Da li genesis mora da zadovolji težinu: ____
- Da li genesis dodeljuje parcelu: ____
- **Heš genesis bloka:** ____

## 6. Težina, parcele i zauzetost

**Težina:** 23 vodeća nulta bita. Bitovi se broje od prvog bajta heša, od najvišeg bita. Heš je ispravan ako su prva 2 bajta `00`, a treći bajt manji od `02`.

| Član | Jezik | Heševa u sekundi |
| --- | --- | --- |
| ____ | C | ~727.000 |
| ____ | C++ | ____ |
| ____ | Rust | ____ |
| ____ | Go | ____ |

**Platno:** 64×32 (64 kolone, 32 reda), parcele 0–2047, red po red: x = p mod 64, y = p / 64. Gornji levi ugao je ____.

**Prva parcela:** p₀ = (256 · heš[30] + heš[31]) mod 2048, gde je heš[0] prvi bajt heša bloka.

**Parcela po bloku:** 1

**Ako je parcela zauzeta:** sledeći pokušaj se dobija iz prethodnog i vremena bloka t (unix timestamp iz bloka):

p₍ᵢ₊₁₎ = (pᵢ · t) mod 2048

Pokušava se dok se ne nađe slobodna parcela, a najviše ____ puta. Ako se ne nađe slobodna (ili su sve zauzete), rudar ne dobija parcelu, a blok je i dalje ispravan.

> ⚠ **Otvoreno pitanje:** pošto je 2048 = 2¹¹, množenje ne prolazi kroz sve parcele. Ako je t paran, pokušaji posle najviše 11 koraka postaju 0 i tu ostaju. Ako je t neparan, pokušaji ostaju samo među parcelama iste parnosti kao p₀. Zato se može desiti da ima slobodnih parcela, a da ih pravilo nikad ne nađe. Grupa treba da potvrdi ili promeni pravilo, npr. sabiranje umesto množenja: p₍ᵢ₊₁₎ = (pᵢ + (t OR 1)) mod 2048, koje sa neparnim korakom prođe kroz svih 2048 parcela za najviše 2048 pokušaja.

**Boja nove parcele pre prvog bojenja:** ____

## 7. Pravila ispravnosti

Svaki čvor radi tačno ove provere, ovim redom.

**Transakcija je ispravna ako:**

1. ____
2. ____
3. ____
4. ____

(Razmislite o: potpisu, vlasništvu parcele u trenutku primene, ponavljanju, opsegu vrednosti, dužinama polja.)

**Blok je ispravan ako:**

1. ____
2. ____
3. ____
4. ____
5. ____

(Razmislite o: težini, prethodnom hešu, visini, vremenu, broju transakcija, ispravnosti svake transakcije redom.)

**Šta čvor radi sa neispravnim blokom ili transakcijom:** ____

## 8. Forkovi

Pobeđuje lanac sa najviše akumuliranog rada.

- Rad jednog bloka: ____ (pošto je težina fiksna, svaki blok ima isti rad, pa pobeđuje duži lanac)
- Rad lanca: ____
- Ako dva lanca imaju isti rad: pobeđuje lanac čiji poslednji blok ima **leksikografski prvi heš**, odnosno manji heš kad se 32 bajta porede redom od prvog. Isto važi za poređenje hex stringova sa malim slovima.
- Šta se dešava sa transakcijama iz blokova koji više nisu na pobedničkom lancu: ____
- Da li čvor čuva blokove sa sporednih forkova: ____

## 9. Mreža i poruke

- Protokol: ____ (predlog: TCP)
- Kako čvor zna adrese ostalih: ____
- Granica poruke: ____ (predlog: jedna poruka po redu, `\n` na kraju)
- Kodiranje teksta: ____
- Najveća dužina poruke: ____

**Vrste poruka:**

| Poruka | Kada se šalje | Sadržaj | Odgovor |
| --- | --- | --- | --- |
| nova transakcija | ____ | ____ | ____ |
| novi blok | ____ | ____ | ____ |
| tražim blok po hešu | ____ | ____ | ____ |
| koji je tvoj poslednji blok | ____ | ____ | ____ |
| ____ | ____ | ____ | ____ |

- Šta čvor radi kad primi blok čiji prethodni blok nema: ____
- Šta čvor radi sa porukom koju ne razume: ____
- Da li se primljeni blokovi i transakcije prosleđuju dalje: ____

## 10. Priključenje, pad i restart

- Šta čvor radi odmah posle pokretanja: ____
- Kako sustigne ostale: ____
- Šta se dešava kad se veza sa nekim čvorom prekine: ____
- Koliko često pokušava ponovo da se poveže: ____
- Timeout za odgovor: ____
- Da li čvor čuva lanac na disku: ____ (dodatni cilj)

## 11. Simulacija laga

Uključena je uvek, osim ako se izričito ne isključi.

- Kašnjenje svake poslate poruke: od ____ ms do ____ ms, raspodela ____
- Verovatnoća da se poruka izgubi: ____ %
- Da li se lag primenjuje po poruci ili po vezi: ____
- Da li poruke na istoj vezi mogu da stignu drugim redom: ____
- Kako se lag uključuje i isključuje: ____

## 12. Test podaci

Svaki primer generiše jedna osoba, a svi ostali ga proveravaju svojim kodom pre prvog povezivanja.

**Provera alata** (poznate vrednosti iz standarda):

| Test | Ulaz | Očekivano |
| --- | --- | --- |
| SHA-256 | `abc` | `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` |
| Ed25519 javni ključ (RFC 8032, test 1) | seed `9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60` | `d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a` |

**Naši test podaci:**

| # | Šta | Ulaz | Bajtovi (hex) | Očekivani heš ili potpis |
| --- | --- | --- | --- | --- |
| 1 | ključ A (seed → javni) | ____ | ____ | ____ |
| 2 | ključ B (seed → javni) | ____ | ____ | ____ |
| 3 | bojenje, potpisuje A | ____ | ____ | ____ |
| 4 | prenos A → B | ____ | ____ | ____ |
| 5 | genesis | ____ | ____ | ____ |
| 6 | blok 1 (rudar A), sa težinom | ____ | ____ | ____ |
| 7 | parcela koju dobija blok 1 | ____ | ____ | ____ |
| 8 | parcela kad je prva zauzeta (pravilo iz sekcije 6) | ____ | ____ | ____ |
| 9 | neispravna transakcija (pogrešan potpisnik) | ____ | ____ | mora biti odbijena |

## Provera pre koda

- [ ] Svako polje `____` je popunjeno
- [ ] Za svaki heš i potpis zapisan je tačan niz bajtova sa dužinama
- [ ] Zapisan je redosled bajtova u brojevima
- [ ] Rešeno je otvoreno pitanje iz sekcije 6 (zauzeta parcela)
- [ ] Genesis heš je izračunat i poklapa se kod svih
- [ ] Svaki član je prošao sve test podatke u svom jeziku
- [ ] Proverili smo da 23 bita daju razumno vreme bloka kad rade svi čvorovi
