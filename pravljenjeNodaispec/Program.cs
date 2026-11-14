using System.Security.Cryptography;
using System.Text;
using NSec.Cryptography;

var alg = SignatureAlgorithm.Ed25519;

// 1. Generisanje ključa (dozvoljen izvoz da bi mogao da ga sačuvaš)
using var key = Key.Create(alg, new KeyCreationParameters
{
    ExportPolicy = KeyExportPolicies.AllowPlaintextExport
});

byte[] pub = key.PublicKey.Export(KeyBlobFormat.RawPublicKey);   // 32 bajta
byte[] priv = key.Export(KeyBlobFormat.RawPrivateKey);            // 32 bajta
Console.WriteLine("Javni ključ: " + Convert.ToHexString(pub).ToLowerInvariant());

// 2. Potpis i provera
byte[] msg = Encoding.UTF8.GetBytes("zdravo");
byte[] sig = alg.Sign(key, msg);                                   // 64 bajta
var pk = PublicKey.Import(alg, pub, KeyBlobFormat.RawPublicKey);
Console.WriteLine("Potpis ispravan: " + alg.Verify(pk, msg, sig));

// 3. SHA-256
byte[] hash = SHA256.HashData(msg);
Console.WriteLine("SHA-256: " + Convert.ToHexString(hash).ToLowerInvariant());