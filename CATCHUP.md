# Pixel Chain — sustizanje (catch-up)

Ovaj dokument opisuje kako čvor dolazi do blokova koje nema: kad se uključi kasnije, posle pada i restarta, ili kad mu se zbog laga izgubi neka poruka. Napisan je prema `node.c`, a uklapa se u `SPEC.md` (sekcije 7–10) i `NETWORK.md`.

## 1. Ukratko

Čvor nikad ne traži „daj mi ceo lanac“. Umesto toga:

1. **Saznaje za vrh** koji ne zna: drugi čvor mu na svake 2 s šalje `tip`.
2. **Traži taj blok** porukom `get_block`.
3. Ako blok ima **nepoznatog roditelja**, čuva ga na čekanju i traži roditelja. Tako ide **unazad**, blok po blok, dok ne stigne do bloka koji već ima (najkasnije genesis).
4. Onda blokove na čekanju **proverava i dodaje unapred**, od najstarijeg ka najnovijem.

Isti mehanizam pokriva sve slučajeve: novi čvor, restart, izgubljene poruke i forkove.

## 2. Delovi sistema

| Deo | Šta čuva | Granica | Gde se koristi |
| --- | --- | --- | --- |
| **Lanac** | svi ispravni blokovi, i sa sporednih grana | memorija | odgovori na `get_block`, izbor vrha |
| **Na čekanju** (pending) | blokovi čiji roditelj još nije poznat, i blokovi iz budućnosti | 1024 bloka | sastavljanje lanca unazad |
| **Traženi** (wanted) | heševi blokova koje želimo, a nemamo | 1024 heša | ponovni zahtevi na 2 s |
| **Mempool** | ispravne transakcije koje još nisu u bloku | 4096 | nove transakcije, ponovne najave |

„Poznat blok“ znači da je u lancu **ili** na čekanju. Poznat blok se nikad ne traži ponovo i ne prosleđuje ponovo.

## 3. Okidači

Sustizanje pokreću tri vrste poruka.

**`tip` sa nepoznatim hešom**

```
primi tip(hash):
    ako hash nije poznat:
        dodaj hash u trazene
        odmah posalji get_block(hash) istom cvoru
```

`tip` se razmenjuje na svake 2 sekunde u svakoj spremnoj sesiji (`get_tip` + `tip`), pa čvor najkasnije za ~2 s sazna da negde postoji vrh koji nema. To je i zaštita od izgubljenih najava blokova.

**`block` sa nepoznatim roditeljem**

```
primi block:
    izracunaj hes
    ako je poznat: kraj
    roditelj (previous) nije u lancu:
        ako hes nema 23 nulta bita: odbaci (smece ne cuvamo)
        sacuvaj blok na cekanju
        dodaj previous u trazene
        odmah posalji get_block(previous) istom cvoru
```

Tako se lanac sastavlja unazad: svaki stigli blok odmah povlači zahtev za svog roditelja.

**`block` čiji roditelj postoji**

Blok se proverava redom iz `SPEC.md` (sekcija 7). Ako je ispravan, dodaje se, prosleđuje svima, i pokreće se **obrada čekanja**:

```
obrada cekanja:
    ponavljaj dok ima napretka:
        za svaki blok na cekanju:
            ako mu je roditelj sada u lancu i nije iz buducnosti:
                skini ga sa cekanja
                proveri i dodaj (isto kao da je upravo stigao)
                ako je dodat: prosledi ga svima; napredak = da
```

Jedan stigli blok tako može da „otključa“ celu nisku blokova na čekanju.

## 4. Primer: novi čvor se uključuje kasno

Čvor **A** radi i ima lanac do visine 5. Čvor **B** se upravo uključio i ima samo genesis (visina 0).

```
B                                              A
|  hello  ------------------------------------>|
|<------------------------------------  hello  |   sesija spremna
|<------------------------------  tip(h5, 5)   |
|  h5 nepoznat -> trazeni = {h5}               |
|  get_block(h5)  ---------------------------->|
|<------------------------------  block(5)     |   roditelj h4 nepoznat
|  cekanje = {5}, trazeni = {h4}               |
|  get_block(h4)  ---------------------------->|
|<------------------------------  block(4)     |   roditelj h3 nepoznat
|  cekanje = {5,4}, trazeni = {h3}             |
|  ... isto za 3, 2 ...                        |
|<------------------------------  block(1)     |   roditelj = genesis, POZNAT
|  dodaj 1 -> obrada cekanja:                  |
|     dodaj 2, 3, 4, 5                         |
|  vrh = 5, isto stanje kao A                  |
```

Svaki korak unazad je jedan povratni put (zahtev i odgovor). Sa lagom od 20–250 ms u svakom smeru to je oko 0,3–0,4 s po bloku. Izmereno sa `node.c` i uključenim lagom: novi čvor je sustigao 18 blokova za 7,4 s, računajući i povezivanje. 100 blokova se tako sustigne za otprilike 30–45 sekundi.

## 5. Izgubljene poruke

Lag odbacuje 10% poruka, pa se zahtev ili odgovor može izgubiti. Za to postoji **ponovno traženje na 2 s**:

```
na svake 2 s:
    uzmi do 4 hesa iz trazenih (kruzno: sledeci put sledeca 4)
    ako je hes u medjuvremenu postao poznat: izbaci ga iz trazenih
    inace: posalji get_block(hes) SVIM spremnim cvorovima
```

- Ako se izgubio `get_block` ili `block`, isti heš se ponovo traži za najviše nekoliko sekundi.
- Zahtev ide **svim** čvorovima, ne samo onom od koga je stigao `tip`. Ako taj čvor u međuvremenu padne, blok stiže od nekog drugog.
- Ako se izgubila najava novog bloka, sledeći `tip` (najkasnije za 2 s) pokazuje da postoji nepoznat vrh.
- Izgubljene najave transakcija se oporavljaju time što svaki čvor na 2 s ponovo najavi do 4 transakcije iz svog mempool-a.

Heš se izbacuje iz traženih čim blok postane poznat, bilo da je dodat u lanac ili stavljen na čekanje.

## 6. Restart posle pada

Lanac se čuva **samo u memoriji** (`SPEC.md` sekcija 10). Na disku su samo ključevi (`data/identity.seed`) i slika platna.

1. Čvor se pokrene i ima samo genesis.
2. Ponovo se poveže sa čvorovima iz `config.txt`, `--peers` ili preko UDP pronalaženja.
3. Dalje ide isto kao u sekciji 4: `hello`, `tip`, pa sustizanje unazad.
4. Pošto je ključ isti, čvor posle sustizanja ponovo vidi svoje parcele kao svoje.

Posledica: ako se ugase **svi** čvorovi, lanac nestaje. Za test oporavka bar jedan čvor mora ostati uključen.

Čvor koji je kraće vreme bio ugašen ima isti problem kao nov: krene od genesis-a i ponovo preuzme sve. Za radionicu to traje desetine sekundi, pa nije problem.

## 7. Forkovi tokom sustizanja

Dok čvor sustiže, ostali i dalje rudare, pa se vrh pomera, a ponekad nastane fork.

- Svaki ispravan blok se čuva, i sa sporedne grane. Čvor zato može da pređe na drugu granu bez ponovnog preuzimanja.
- Posle svakog dodatog bloka bira se najbolji vrh: veća visina, a pri istoj visini manji heš (`SPEC.md` sekcija 8).
- Ako se vrh promeni na drugu granu (reorganizacija):
  - stanje platna se ponovo izračuna od genesis-a po novoj grani;
  - transakcije iz blokova napuštene grane vraćaju se u mempool, ako su i dalje ispravne;
  - rudarenje kreće ispočetka na novom vrhu.
- `get_block` odgovara za svaki poznat blok, i sa sporedne grane, pa i drugi čvorovi mogu da sustignu bilo koju granu.

## 8. Blokovi iz budućnosti

Blok čiji je `timestamp` više od 120 s ispred lokalnog sata nije neispravan, već **odložen**:

- čuva se na čekanju i ne prosleđuje se;
- na svake 2 s i posle svakog novog bloka ponovo se proba;
- čim postane prihvatljiv (sat sustigne), proverava se i dodaje normalno.

Zato je dobro da računari imaju tačno vreme (uključena automatska sinhronizacija sata u Windows-u).

## 9. Granice i šta se dešava kad se popune

| Granica | Šta se dešava |
| --- | --- |
| 1024 bloka na čekanju | izbacuje se blok sa **najvećom visinom**; preci imaju prednost jer su potrebni da bi se išta dodalo. Izbačeni potomci se kasnije ponovo pronađu preko `tip`. |
| 1024 tražena heša | izbacuje se najstariji zahtev |
| 128 poruka u redu za slanje jedne sesije | sesija se zatvara, pa ponovo otvara posle 2 s; sustizanje se nastavlja |
| 30 s bez ijednog bajta u sesiji | sesija se zatvara i ponovo otvara |

Ako je čvor više od 1024 bloka iza, sustizanje i dalje uspeva, samo sporije: preci se dodaju u lanac, a izbačeni noviji blokovi se ponovo zatraže kad stigne sledeći `tip`.

## 10. Kako proveriti da radi

**Kasno uključenje:**

1. Pokreni čvor 1 sa rudarenjem i sačekaj desetak blokova.
2. Pokreni čvor 2.
3. U interfejsu čvora 2 (`http://localhost:8002`) visina treba za par sekundi da skoči na istu visinu kao kod čvora 1, a vrh treba da bude isti.
4. U panelu „Poruke preko mreže“ vidi se da je čvor 2 slao `get_block` i primao `block`.

**Pad i restart:**

1. Pokreni dva čvora sa rudarenjem.
2. Ugasi jedan sa Ctrl+C (ili zatvori terminal) i ostavi drugi da rudari još malo.
3. Ponovo pokreni ugašeni čvor.
4. Za par sekundi oba moraju imati isti vrh i isto **STANJE**, a ponovo pokrenuti čvor svoje stare parcele.

**Završni test** (`SPEC` radionice): zaustavi rudarenje svuda (`mine=0` u `config.txt`, ili komanda `mine off`), sačekaj nekoliko sekundi, pa uporedi vrh i stanje na svim čvorovima. Moraju biti isti.

## 11. Šta bi moglo bolje (nije urađeno)

- **Brže preuzimanje:** sada se ide blok po blok unazad. Mogla bi se dodati poruka koja traži više blokova odjednom, ali to bi bila promena protokola, pa je mora prihvatiti cela grupa.
- **Čuvanje lanca na disku:** posle restarta čvor ne bi morao sve ponovo da preuzima (dodatni cilj radionice).
