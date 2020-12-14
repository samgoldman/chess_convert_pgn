#[macro_use] extern crate lazy_static;

use clap::{Arg, App};
use std::io::{self};
use std::io::prelude::*;
use std::fs::File;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use regex::Regex;

#[allow(non_snake_case)]
#[path = "../target/flatbuffers/chess_generated.rs"]
pub mod chess_flatbuffers;

pub use chess_flatbuffers::chess::{Game, GameArgs, GameList, GameListArgs, GameResult, Termination, Piece, NAG, File as BoardFile, Check};

// https://stackoverflow.com/questions/45882329/read-large-files-line-by-line-in-rust
mod file_reader {
    use std::{
        fs::File,
        io::{self, prelude::*},
    };

    pub struct BufReader {
        reader: io::BufReader<File>,
    }

    impl BufReader {
        pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
            let file = File::open(path)?;
            let reader = io::BufReader::new(file);

            Ok(Self { reader })
        }

        pub fn read_line<'buf>(
            &mut self,
            buffer: &'buf mut String,
        ) -> Option<io::Result<&'buf mut String>> {
            buffer.clear();

            self.reader
                .read_line(buffer)
                .map(|u| if u == 0 { None } else { Some(buffer) })
                .transpose()
        }
    }
}

pub struct Converter<'a> {
    reader: file_reader::BufReader,
    builder: FlatBufferBuilder<'a>,
    game_args: GameArgs<'a>,
    games: Vec<WIPOffset<Game<'a>>>
}

impl<'a> Converter<'a> {
    fn read_header(&mut self, line: &str) {
        lazy_static! {
            static ref RE: Regex = Regex::new(r#"\[(.*) "(.*)"\]"#).unwrap();
        }

        for cap in RE.captures_iter(line) {
            let field = &cap[1];
            let value = &cap[2];

            match field {
                "Opening" => {
                    self.game_args.opening = Some(self.builder.create_string(value));
                },
                "UTCDate" => {
                    let date_parts: Vec<&str> = value.split('.').collect();

                    self.game_args.year = date_parts[0].parse::<u16>().unwrap();
                    self.game_args.month = date_parts[1].parse::<u8>().unwrap();
                    self.game_args.day = date_parts[2].parse::<u8>().unwrap();
                }, 
                "TimeControl" => {
                    if value == "-" {
                        self.game_args.time_control_main = 0;
                        self.game_args.time_control_increment = 0;
                    } else {
                        let time_control_parts: Vec<&str> = value.split("+").collect();
                        self.game_args.time_control_main = time_control_parts[0].parse::<u16>().unwrap();
                        self.game_args.time_control_increment = time_control_parts[1].parse::<u8>().unwrap();
                    }
                },
                "WhiteElo" => {
                    if value == "?" {
                        self.game_args.white_rating = 0;
                    } else {
                        self.game_args.white_rating = value.parse::<u16>().unwrap();
                    }
                }, 
                "BlackElo" => {
                    if value == "?" {
                        self.game_args.black_rating = 0;
                    } else {
                        self.game_args.black_rating = value.parse::<u16>().unwrap();
                    }
                },
                "WhiteRatingDiff" => {
                    self.game_args.white_diff = value.parse::<i16>().unwrap();
                }, 
                "BlackRatingDiff" => {
                    self.game_args.black_diff = value.parse::<i16>().unwrap();
                },
                "ECO" => {
                    if value == "?" {
                        self.game_args.eco_category = 0;
                        self.game_args.eco_subcategory = 0;
                    } else {
                        let cat_char = (&value[..1]).chars().next().unwrap();

                        let mut cat_char_vec: Vec<u8> = vec![0];
                        cat_char.encode_utf8(&mut cat_char_vec);

                        self.game_args.eco_category = cat_char_vec[0] as i8;
                        self.game_args.eco_subcategory = (&value[1..]).parse::<u8>().unwrap();
                    }
                }, 
                "Result" => {
                    self.game_args.result = match value {
                        "1-0" => GameResult::White,
                        "0-1" => GameResult::Black,
                        "1/2-1/2" => GameResult::Draw,
                        "*" => GameResult::Star,
                        u => panic!("Unknown result: {}", u)
                    }
                }
                "Termination" => {
                    self.game_args.termination = match value {
                        "Normal" => Termination::Normal,
                        "Time forfeit" => Termination::TimeForfeit,
                        "Abandoned" => Termination::Abandoned,
                        "Rules infraction" => Termination::RulesInfraction,
                        "Unterminated" => Termination::Unterminated,
                        u => panic!("Unknown termination: {}", u)
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_game_text(&mut self, line: &str) {
        lazy_static! {
            static ref _RE_EVAL_INDICATOR: Regex = Regex::new(r#"eval"#).unwrap();
            static ref _RE_CLK_INDICATOR: Regex = Regex::new(r#"clk"#).unwrap();
            static ref RE_MOVE: Regex = Regex::new(r#"^([NBRQK]?)([a-h1-9]{0,4})(x?)([a-h1-9]{2})(=?)([NBRQK]?)([+#]?)([?!]{0,2})$"#).unwrap();
            static ref RE_COORD: Regex = Regex::new(r#"^([a-h]?)([1-8]?)$"#).unwrap();
        }

        let tokens = line.split(" ");

        let mut pieces_moved: Vec<Piece> = vec![];
        let mut captured: Vec<bool> = vec![];
        let mut promotions: Vec<Piece> = vec![];
        let mut nags: Vec<NAG> = vec![];
        let mut checks: Vec<Check> = vec![];

        // let mut from_files: Vec<BoardFile> = vec![];
        let mut to_files: Vec<BoardFile> = vec![];

        // let mut from_ranks: Vec<u8> = vec![];
        let mut to_ranks: Vec<u8> = vec![];

        for token in tokens {
            for cap in RE_MOVE.captures_iter(token) {
                let piece_str = &cap[1];
                let disambiguation_str = &cap[2];
                let capture_str = &cap[3];
                let dest_str = &cap[4];     
                assert!(disambiguation_str.len() <= dest_str.len());
                let promotion_str = &cap[5];
                let promotion_piece = &cap[6];
                assert!(promotion_piece.len() == promotion_str.len());
                let check_str = &cap[7];
                let nag_str = &cap[8];

                // for coord_cap in RE_COORD.captures_iter(disambiguation_str) {
                //     from_files.push(match &coord_cap[1] {
                //         "" => BoardFile::N_A,
                //         "a" => BoardFile::A, 
                //         "b" => BoardFile::B, 
                //         "c" => BoardFile::C, 
                //         "d" => BoardFile::D, 
                //         "e" => BoardFile::E, 
                //         "f" => BoardFile::F, 
                //         "g" => BoardFile::G, 
                //         "h" => BoardFile::H, 
                //         u => panic!("Unrecongnized file: {}", u), 
                //     });

                //     from_ranks.push(match &coord_cap[2] {
                //         "" => 0,
                //         "1" => 1, 
                //         "2" => 2, 
                //         "3" => 3, 
                //         "4" => 4, 
                //         "5" => 5, 
                //         "6" => 6, 
                //         "7" => 7, 
                //         "8" => 8, 
                //         u => panic!("Unrecongnized rank: {}", u), 
                //     });
                // }

                for coord_cap in RE_COORD.captures_iter(dest_str) {
                    to_files.push(match &coord_cap[1] {
                        "" => BoardFile::N_A,
                        "a" => BoardFile::A, 
                        "b" => BoardFile::B, 
                        "c" => BoardFile::C, 
                        "d" => BoardFile::D, 
                        "e" => BoardFile::E, 
                        "f" => BoardFile::F, 
                        "g" => BoardFile::G, 
                        "h" => BoardFile::H, 
                        u => panic!("Unrecongnized file: {}", u), 
                    });

                    to_ranks.push(match &coord_cap[2] {
                        "" => 0,
                        "1" => 1, 
                        "2" => 2, 
                        "3" => 3, 
                        "4" => 4, 
                        "5" => 5, 
                        "6" => 6, 
                        "7" => 7, 
                        "8" => 8, 
                        u => panic!("Unrecongnized rank: {}", u), 
                    });
                }

                pieces_moved.push(match piece_str {
                    "" => Piece::Pawn,
                    "N" => Piece::Knight,
                    "B" => Piece::Bishop,
                    "R" => Piece::Rook,
                    "Q" => Piece::Queen,
                    "K" => Piece::King,
                    u => panic!("Unrecongized piece: {}", u)
                });

                captured.push(match capture_str {
                    "" => false,
                    "x" => true,
                    u => panic!("Unreconized capture flag: {}", u)
                });

                promotions.push(match promotion_piece {
                    "" => Piece::None,
                    "N" => Piece::Knight,
                    "B" => Piece::Bishop,
                    "R" => Piece::Rook,
                    "Q" => Piece::Queen,
                    "K" => Piece::King,
                    u => panic!("Unrecongized promotion piece: {}", u)
                });

                nags.push(match nag_str {
                    "" => NAG::None,
                    "!" => NAG::Good,
                    "?" => NAG::Mistake,
                    "!!" => NAG::Brilliant,
                    "??" => NAG::Blunder,
                    "!?" => NAG::Speculative,
                    "?!" => NAG::Dubious,
                    _ => NAG::Unrecognized
                });

                checks.push(match check_str {
                    "" => Check::None,
                    "+" => Check::Check,
                    "#" => Check::Mate,
                    u => panic!("Unrecongized check flag: {}", u)
                })
            }
        }

        self.game_args.moved = Some(self.builder.create_vector(&pieces_moved));
        self.game_args.captured = Some(self.builder.create_vector(&captured));
        self.game_args.promotions = Some(self.builder.create_vector(&promotions));
        self.game_args.nags = Some(self.builder.create_vector(&nags));
        self.game_args.checks = Some(self.builder.create_vector(&checks));

        //self.game_args.from_files = Some(self.builder.create_vector(&from_files));
        self.game_args.to_files = Some(self.builder.create_vector(&to_files));

        //self.game_args.from_ranks = Some(self.builder.create_vector(&from_ranks));
        self.game_args.to_ranks = Some(self.builder.create_vector(&to_ranks));
    }

    fn convert_next_game(&mut self) -> std::io::Result<bool> {
        let mut buffer = String::new();

        self.game_args = GameArgs{
            ..Default::default()
        };

        loop {
            let res = self.reader.read_line(&mut buffer);

            match res {
                None => return Ok(false),
                Some(line) => {
                    let trimmed = line?.trim();
                    if trimmed.len() > 1 && trimmed.chars().nth(0).unwrap() == '[' {
                        self.read_header(trimmed);
                    } else {
                        assert!(trimmed == ""); 
                        break;
                    }
                }
            }
        }

        let game_text = self.reader.read_line(&mut buffer).unwrap()?;
        self.parse_game_text(game_text.trim());

        let line = self.reader.read_line(&mut buffer).unwrap()?;
        assert!(line.trim() == "");

        let game = Game::create(&mut self.builder, &self.game_args);
        self.games.push(game);
        
        Ok(true)
    }

    fn save_to_list(&mut self) -> &[u8] {
        let vectored_games = Some(self.builder.create_vector(&self.games));
        let game_list = GameList::create(&mut self.builder, &GameListArgs{
            games: vectored_games
        });

        self.games = vec![];

        self.builder.finish(game_list, None);
        self.builder.finished_data()
    }
}

fn main() -> io::Result<()> {
    let matches = App::new("PGN to Flat Buffer")
        .version("0.1.0")
        .author("Sam Goldman")
        .about("Convert Lichess PGN files to flat buffers")
        .arg(Arg::with_name("input_file")
            .short("i")
            .long("input_file")
            .takes_value(true)
            .help("The PGN to parse").required(true))
        .arg(Arg::with_name("output_prefix").short("o").long("output_prefix").takes_value(true).required(true))
        .arg(Arg::with_name("max").short("m").long("max").takes_value(true).default_value("10000").help("The number of games to put in each buffer"))
        .get_matches();

    let input_file = matches.value_of("input_file").unwrap();
    let output_prefix = matches.value_of("output_prefix").unwrap();
    let max = matches.value_of("max").unwrap().parse::<u32>().unwrap();

    let mut converter = Converter {
        reader: file_reader::BufReader::open(input_file)?,
        builder: flatbuffers::FlatBufferBuilder::new_with_capacity(1024*1024),
        game_args: GameArgs{
            ..Default::default()
        },
        games: vec![]
    };

    let mut i = 0;
    let mut k = 0;
    loop {
        let res = converter.convert_next_game()?;
        if false == res {
             break;
        } else {
            i += 1;
            if i == max {
                let data = converter.save_to_list();
    
                let mut pos = 0;
                let mut buffer = File::create(format!("{}_{:06}.bin", output_prefix, k))?;
            
                while pos < data.len() {
                    let bytes_written = buffer.write(&data[pos..])?;
                    pos += bytes_written;
                }

                converter.builder = flatbuffers::FlatBufferBuilder::new_with_capacity(1024*1024);

                i = 0;
                k += 1;
            }
        }
    }

    if i > 0 {
        let data = converter.save_to_list();
        
        let mut pos = 0;
        let mut buffer = File::create(format!("{}_{:06}.bin", output_prefix, k))?;

        while pos < data.len() {
            let bytes_written = buffer.write(&data[pos..])?;
            pos += bytes_written;
        }
    }

    Ok(())
}
