pub fn dex_node_grpc_var(mut network: String) -> String {
    network.make_ascii_uppercase();

    if const { SEPARATOR_CHAR != '-' } {
        while let Some(index) = network.find('-') {
            network.replace_range(index..=index, SEPARATOR_STR);
        }
    }

    network.reserve_exact(VAR_SUFFIX.len());

    network.push_str(VAR_SUFFIX);

    network
}

const SEPARATOR_CHAR: char = '_';

const SEPARATOR_STR: &str = {
    const BYTES: [u8; SEPARATOR_CHAR.len_utf8()] = {
        let mut bytes = [0; SEPARATOR_CHAR.len_utf8()];

        SEPARATOR_CHAR.encode_utf8(&mut bytes);

        bytes
    };

    if let Ok(s) = core::str::from_utf8(&BYTES) {
        s
    } else {
        panic!("Separator should be valid UTF-8!")
    }
};

const VAR_SUFFIX: &str = {
    const SEGMENTS: &[&str] = &["NODE", "GRPC"];

    const LENGTH: usize = {
        let mut sum = (SEGMENTS.len() + 1) * SEPARATOR_STR.len();

        let mut index = 0;

        while index < SEGMENTS.len() {
            sum += SEGMENTS[index].len();

            index += 1;
        }

        sum
    };

    const BYTES: [u8; LENGTH] = {
        const fn write_bytes(
            destination: &mut [u8; LENGTH],
            mut destination_index: usize,
            source: &[u8],
        ) -> usize {
            let mut source_index = 0;

            while source_index < source.len() {
                destination[destination_index] = source[source_index];

                destination_index += 1;

                source_index += 1;
            }

            destination_index
        }

        #[inline]
        const fn write_separator(
            destination: &mut [u8; LENGTH],
            index: usize,
        ) -> usize {
            write_bytes(destination, index, SEPARATOR_STR.as_bytes())
        }

        let mut bytes = [0; LENGTH];

        let mut byte_index = write_separator(&mut bytes, 0);

        let mut index = 0;

        while index < SEGMENTS.len() {
            byte_index = write_separator(&mut bytes, byte_index);

            byte_index =
                write_bytes(&mut bytes, byte_index, SEGMENTS[index].as_bytes());

            index += 1;
        }

        bytes
    };

    if let Ok(s) = core::str::from_utf8(&BYTES) {
        s
    } else {
        panic!("Environment variable name suffix should be valid UTF-8!")
    }
};

#[test]
fn variable_name_generation() {
    assert_eq!(VAR_SUFFIX, "__NODE_GRPC");

    assert_eq!(
        dex_node_grpc_var("AbBCD_e-Fg-H-i".into()),
        "ABBCD_E_FG_H_I__NODE_GRPC"
    );
}
