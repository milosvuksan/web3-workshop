import java.security.MessageDigest;
import java.util.HexFormat;
public class HashBench {
    static void run(int n, byte[] buf, byte[] out, MessageDigest md) throws Exception {
        for (long i=0; i<n; i++) {
            for (int j=0; j<8; j++) buf[buf.length-8+j] = (byte)(i >>> (8*j));
            md.update(buf); md.digest(out, 0, out.length);
        }
    }
    public static void main(String[] args) throws Exception {
        int n = Integer.parseInt(args[0]), size = Integer.parseInt(args[1]);
        byte[] buf = new byte[size], out = new byte[32];
        for (int j=0; j<size; j++) buf[j] = (byte)j;
        MessageDigest md = MessageDigest.getInstance("SHA-256");
        run(200000, buf, out, md);
        long start = System.nanoTime();
        run(n, buf, out, md);
        double elapsed = (System.nanoTime() - start) / 1e9;
        System.out.printf(java.util.Locale.ROOT,
            "{\"seconds\":%.9f,\"hashes_per_second\":%.2f,\"digest\":\"%s\"}%n",
            elapsed, n/elapsed, HexFormat.of().formatHex(out));
    }
}
