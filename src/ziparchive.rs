#![allow(dead_code)]
use std::fs::File;
use std::path::Path;
use std::io::Read;
use std::io::Cursor;
use std::io::Seek;
use std::slice;
use std::mem;
use std::io::SeekFrom;

/// Marks the start of a file, and provides the uncompressed data
struct LocalFileHeader {            
    
                                    // OFFSETS:
    magic_number: u32,              // 0            0x04034b50 (read as a little-endian number)
    version_needed: u16,            // 4
    spacer_unused: u16,             // 6
    compression_method: u16,        // 8
    last_modify_time: u16,          // 10
    last_modify_date: u16,          // 12
    crc32_uncompressed: u32,        // 14
    compressed_size: u32,           // 18
    uncompressed_size: u32,         // 22
    file_name_length: u16,          // 26 (n)
    extra_field_length: u16,        // 28 (m)
    file_name: Vec<u8>,             // 30
    extra_field: Vec<u8>,           // 30 + n
    compressed_data: Vec<u8>
    // https://en.wikipedia.org/wiki/Zip_(file_format)
}

/// The central directory record (CDR) is an expanded form of the local header
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct CentralDirectoryFileHeader {
    /// The Central Directory Contains multiple CDRs     
                                        // OFFSETS
    magic_number: u32,                  // 0        0x02014b50 (Central directory file header signature)
    version_made_by: u16,               // 4
    version_needed: u16,                // 6
    spacer_unused: u16,                 // 8
    compression_method: u16,            // 10
    last_modify_time: u16,              // 12
    last_modify_date: u16,              // 14
    crc32_uncompressed: u32,            // 16
    compressed_size: u32,               // 20
    uncompressed_size: u32,             // 24
    file_name_length: u16,              // 28       (n)
    extra_field_length: u16,            // 30       (m)
    file_comment_length: u16,           // 32       (k)
    disk_number_source: u16,            // 34
    internal_file_attributes: u16,      // 36
    external_file_attributes: u32,      // 38
    relative_offset_localheader: u32,   // 42       Relative offset of local file header. This is the number of bytes between the start of the first disk on which the file occurs, and the start of the local file header.
    // filename: Vec<u8>,                  // 46
    // extra_field: Vec<u8>,               // 46 + n
    // file_comment: Vec<u8>               // 46 + n + m
}

/// After all the central directory entries comes the end of central directory (EOCD) record, which marks the end of the ZIP file
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct EndOfCentralDirectoryRecord {


                                        // OFFSETS
    magic_number: u32,                  // 0        0x06054b50
    number_of_current_disk: u16,        // 4
    disk_where_cdr_starts: u16,         // 6
    num_cdr_on_disk: u16,               // 8
    total_cdr: u16,                     // 10
    size_of_cdr: u32,                   // 12       Size of the Central Directory in Bytes
    offset_cdr_start: u32,              // 16       Offset from the start of the archive where the CentralDirectory starts (in bytes, obvi)
    comment_length: u16,                // 20       (n)
    // comment: Vec<u8>                 Moved to wrapper EofRecord
}

#[derive(Debug, Clone)]
/// Wrapper around EndOfCentralDirectoryRecord that allows us to manually fill the variably sized data
struct EofRecord {
    static_data: EndOfCentralDirectoryRecord,
    start_offset: u64,
    end_offset: u64,
    comment: Vec<u8>,
}

impl EofRecord {
    pub fn new(mut file: &std::fs::File, offset_starting: u64) -> EofRecord {
        let mut static_data = EndOfCentralDirectoryRecord::new();
        let end_offset = static_data.load_data(&mut file, offset_starting);
        let mut comment_buf = vec![0; static_data.comment_length as usize];
        file.seek(SeekFrom::Start(end_offset)).expect("Couldn't seek to EOF comment");
        file.read(&mut comment_buf).expect("Error reading EOF comment");

        return EofRecord{
            static_data: static_data,
            start_offset: offset_starting,
            end_offset: end_offset,
            comment: comment_buf
        }
        
    }
}

impl EndOfCentralDirectoryRecord {
    /// Reads a binary array into a struct, using the C representaion
    /// Returns a offset of where the reading ended
    /// https://stackoverflow.com/questions/25410028/how-to-read-a-struct-from-a-file-in-rust
    pub fn load_data(&mut self, mut file: &std::fs::File, offset_starting: u64) -> u64{
        println!("Loading EOF Record from offset: {:#X}", offset_starting);
        let data_size = mem::size_of::<EndOfCentralDirectoryRecord>();
        let mut struct_data = vec![0u8; data_size];

        file.seek(SeekFrom::Start(offset_starting)).unwrap();
        file.read(&mut struct_data).unwrap();

        let mut data: EndOfCentralDirectoryRecord = unsafe {mem::zeroed()};
        

        let mut c = Cursor::new(struct_data);

        unsafe {
            let data_slice = slice::from_raw_parts_mut(&mut data as *mut _ as *mut u8, data_size);
            c.read_exact(data_slice).unwrap();
        }

        self.magic_number = data.magic_number;
        self.number_of_current_disk = data.number_of_current_disk;
        self.disk_where_cdr_starts = data.disk_where_cdr_starts;
        self.num_cdr_on_disk = data.num_cdr_on_disk;
        self.total_cdr = data.total_cdr;
        self.size_of_cdr = data.size_of_cdr;
        self.offset_cdr_start = data.offset_cdr_start;
        self.comment_length = data.comment_length;

        return offset_starting + data_size as u64;
    }

    pub fn new() -> EndOfCentralDirectoryRecord{
        EndOfCentralDirectoryRecord{
            magic_number: 0x06054b50,
            number_of_current_disk: 0,
            disk_where_cdr_starts: 0,
            num_cdr_on_disk: 0,
            total_cdr: 0,
            size_of_cdr: 0,
            offset_cdr_start: 0,
            comment_length: 0
        }
    }
}

pub struct ZipArchive {
    local_file_data: Vec<LocalFileHeader>,
    central_records: Vec<CentralDirectoryFileHeader>,
    eof_record: EofRecord
}


impl ZipArchive {
    pub fn new(filename: &str) -> ZipArchive{
        println!("New ZipArchive! {}", filename);
        let path = Path::new(filename);
        let mut file = match File::open(path) {
            Err(why) => panic!("Couldn't open {}: {}", path.display(), why.to_string()),
            Ok(file) => file
        };

        let last_pos = match file.seek(SeekFrom::End(0)) {
            Err(why) => panic!("Couldn't seek! {}", why.to_string()),
            Ok(pos) => pos
        };

        let eof_record_num:[u8; 4] = [0x50, 0x4b, 0x05, 0x06]; // 0x06054b50 Reversed for lil-endian

        let mut current_index: i64 = 1;
        while current_index < last_pos as i64 { // basically, this loop moves the read position back 1 byte at a time from the end, until our
            // four-byte buffer looks like the eof_record_num, which means we have found the start of the EOF record.
            let mut buffer: [u8; 4] = [0x0; 4];
            file.seek(SeekFrom::End(-current_index)).unwrap();
            file.read(&mut buffer[..]).unwrap();
            if &eof_record_num[..] == &buffer[..] {
                println!("Found magic number for EOF structure at offset {:#X}", last_pos-current_index as u64);
                break;
            }
            current_index = current_index + 1;
        }

        let eofdirectory_offset: u64 = last_pos - current_index as u64;

        return ZipArchive{
            local_file_data: Vec::new(),
            central_records: Vec::new(),
            eof_record: EofRecord::new(&mut file, eofdirectory_offset)
        };
    }

    pub fn print_eof(self){
        println!("EofRecord: {:#?}", self.eof_record);
    }
}