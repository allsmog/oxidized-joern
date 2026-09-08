signed long identity(signed long value) { signed long local = value; return local; }
signed long declared(signed long value);
signed long call_definition(signed long value) { return identity(value); }
signed long call_prototype(signed long value) { return declared(value); }
