long long identity(long long value) { long long local = value; return local; }
long long declared(long long value);
long long call_definition(long long value) { return identity(value); }
long long call_prototype(long long value) { return declared(value); }
