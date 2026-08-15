final class Main {
  static String source(String value) {
    return value;
  }

  static String transform(String value) {
    return value;
  }

  static void sink(String value) {
    System.out.println(value);
  }

  static void main(String user) {
    String raw = source(user);
    String clean = transform(raw);
    sink(clean);
  }
}
