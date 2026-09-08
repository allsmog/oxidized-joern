long identity(long value) { long local = value; return local; }
long declared(long value);
long call_definition(long value) { return identity(value); }
long call_prototype(long value) { return declared(value); }
