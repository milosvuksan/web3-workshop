using System.Buffers.Binary;
using System.Diagnostics;
using System.Security.Cryptography;
using System.Text.Json;

int n = int.Parse(args[0]);
int size = int.Parse(args[1]);
bool incremental = args.Length > 2 && args[2] == "incremental";
byte[] buf = new byte[size];
byte[] output = new byte[32];
for (int j = 0; j < size; j++) buf[j] = (byte)j;
using var hasher = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);

void Run(int count)
{
    if (incremental)
    {
        for (int i = 0; i < count; i++)
        {
            BinaryPrimitives.WriteUInt64LittleEndian(buf.AsSpan(size - 8), (ulong)i);
            hasher.AppendData(buf);
            hasher.GetHashAndReset(output);
        }
    }
    else
    {
        for (int i = 0; i < count; i++)
        {
            BinaryPrimitives.WriteUInt64LittleEndian(buf.AsSpan(size - 8), (ulong)i);
            SHA256.HashData(buf, output);
        }
    }
}

Run(200000);
var stopwatch = Stopwatch.StartNew();
Run(n);
stopwatch.Stop();
double elapsed = stopwatch.Elapsed.TotalSeconds;
Console.WriteLine(JsonSerializer.Serialize(new {
    seconds = elapsed,
    hashes_per_second = n / elapsed,
    digest = Convert.ToHexString(output).ToLowerInvariant()
}));
