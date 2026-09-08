signed long long identity(signed long long value) { signed long long local = value; return local; }
signed long long declared(signed long long value);
signed long long call_definition(signed long long value) { return identity(value); }
signed long long call_prototype(signed long long value) { return declared(value); }
