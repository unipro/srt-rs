// Packet structures
// see https://tools.ietf.org/html/draft-gg-udt-03#page-5

use bytes::{Buf, BufMut, BytesMut};

use byteorder::BigEndian;

use std::io::{Cursor, Error, ErrorKind, Result};
use std::net::IpAddr;

/// Represents A UDT/SRT packet
#[derive(Debug)]
pub enum Packet {
    Data(DataPacket),
    Control(ControlPacket),
}

/// A UDT packet carrying data
///  0                   1                   2                   3
///  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |0|                     Packet Sequence Number                  |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |FF |O|                     Message Number                      |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |                          Time Stamp                           |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |                    Destination Socket ID                      |
///    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// (from https://tools.ietf.org/html/draft-gg-udt-03#page-)
#[derive(Debug)]
pub struct DataPacket {
    /// The sequence number is packet based, so if packet n has
    /// sequence number `i`, the next would have `i + 1`

    /// Represented by a 31 bit unsigned integer, so
    /// Sequence number is wrapped after it recahed 2^31 - 1
    pub seq_number: i32,

    /// Message location
    /// Represented by the first two bits in the second row of 4 bytes
    pub message_loc: PacketLocation,

    /// Should this message be delivered in order?
    /// Represented by the third bit in the second row
    pub in_order_delivery: bool,

    /// The message number, is the ID of the message being passed
    /// Represented by the final 29 bits of the third row
    /// It's only 29 bits long, so it's wrapped after 2^29 - 1
    pub message_number: i32,

    /// The timestamp, relative to when the connection was created.
    pub timestamp: i32,

    /// The dest socket id, used for UDP multiplexing
    pub dest_sockid: i32,

    /// The rest of the packet, the payload
    pub payload: BytesMut,
}

/// Signifies the packet location in a message for a data packet
#[derive(Debug, PartialEq)]
pub enum PacketLocation {
    /// The first packet in a message, 10 in the FF location
    First,

    /// Somewhere in the middle, 00 in the FF location
    Middle,

    /// The last packet in a message, 01 in the FF location
    Last,

    /// The only packet in a message, 11 in the FF location
    Only,
}

impl PacketLocation {
    // Takes the second line of a data packet and gives the packet location in the message
    fn from_i32(from: i32) -> PacketLocation {
        match from {
            x if (x & (0b10 << 30)) == (0b10 << 30) => PacketLocation::First,
            x if (x & (0b01 << 30)) == (0b01 << 30) => PacketLocation::Last,
            x if (x & (0b11 << 30)) != (0b11 << 30) => PacketLocation::Only,
            _ => PacketLocation::Middle,
        }
    }

    fn to_i32(&self) -> i32 {
        match self {
            &PacketLocation::First => 0b11 << 30,
            &PacketLocation::Middle => 0b10 << 30,
            &PacketLocation::Last => 0b01 << 30,
            &PacketLocation::Only => 0b00,
        }
    }
}

/// A UDP packet carrying control information
///  0                   1                   2                   3
///  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |1|             Type            |            Reserved           |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |     |                    Additional Info                      |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |                            Time Stamp                         |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |                    Destination Socket ID                      |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
///  |                                                               |
///  ~                 Control Information Field                     ~
///  |                                                               |
///  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// (from https://tools.ietf.org/html/draft-gg-udt-03#page-5)
#[derive(Debug)]
pub struct ControlPacket {
    /// The timestamp, relative to the socket start time
    pub timestamp: i32,

    /// The dest socket ID, used for multiplexing
    pub dest_sockid: i32,

    /// The extra data
    pub control_type: ControlTypes,
}

/// The different kind of control packets
#[derive(Debug)]
pub enum ControlTypes {
    /// The control packet for initiating connections, type 0x0
    /// Does not use Additional Info
    Handshake(HandshakeControlInfo),

    /// To keep a connection alive
    /// Does not use Additional Info or Control Info, type 0x1
    KeepAlive,

    /// ACK packet, type 0x2
    /// Additional Info (the i32) is the ACK sequence number
    Ack(i32, AckControlInfo),

    /// NAK packet, type 0x3
    /// Additional Info isn't used
    Nak(NakControlInfo),

    /// Shutdown packet, type 0x5
    Shutdown,

    /// Acknowldegement of Acknowldegement (ACK2) 0x6
    /// Additinal Info (the i32) is the ACK sequence number to acknowldege
    Ack2(i32),

    /// Drop request, type 0x7
    /// Additinal Info (the i32) is the message ID to drop
    DropRequest(i32, DropRequestControlInfo),
}

impl ControlTypes {
    /// Deserialize a control info
    /// packet_type: The packet ID byte, the second byte in the second row
    fn deserialize<T: Buf>(packet_type: u8, extra_info: i32, mut buf: T) -> Result<ControlTypes> {
        match packet_type {
            0x0 => {
                // Handshake

                let udt_version = buf.get_i32::<BigEndian>();
                let sock_type = SocketType::from_i32(buf.get_i32::<BigEndian>())?;
                let init_seq_num = buf.get_i32::<BigEndian>();
                let max_packet_size = buf.get_i32::<BigEndian>();
                let max_flow_size = buf.get_i32::<BigEndian>();
                let connection_type = ConnectionType::from_i32(buf.get_i32::<BigEndian>())?;
                let socket_id = buf.get_i32::<BigEndian>();
                let syn_cookie = buf.get_i32::<BigEndian>();

                // get the IP
                let mut ip_buf: [u8; 16] = [0; 16];
                buf.copy_to_slice(&mut ip_buf);
                let peer_addr = IpAddr::from(ip_buf);

                Ok(ControlTypes::Handshake(HandshakeControlInfo {
                    udt_version,
                    sock_type,
                    init_seq_num,
                    max_packet_size,
                    max_flow_size,
                    connection_type,
                    socket_id,
                    syn_cookie,
                    peer_addr,
                }))
            }
            0x1 => Ok(ControlTypes::KeepAlive),
            0x2 => {
                // ACK

                // read control info
                let recvd_until = buf.get_i32::<BigEndian>();

                // if there is more data, use it. However, it's optional
                let mut opt_read_next = move || {
                    if buf.remaining() > 4 {
                        Some(buf.get_i32::<BigEndian>())
                    } else {
                        None
                    }
                };
                let rtt = opt_read_next();
                let rtt_variance = opt_read_next();
                let buffer_available = opt_read_next();
                let packet_recv_rate = opt_read_next();
                let est_link_cap = opt_read_next();

                Ok(ControlTypes::Ack(
                    extra_info,
                    AckControlInfo {
                        recvd_until,
                        rtt,
                        rtt_variance,
                        buffer_available,
                        packet_recv_rate,
                        est_link_cap,
                    },
                ))
            }
            0x3 => {
                // NAK

                unimplemented!()
            }
            0x5 => Ok(ControlTypes::Shutdown),
            0x6 => {
                // ACK2

                unimplemented!()
            }
            0x7 => {
                // Drop request

                unimplemented!()
            }
            x => {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("Unrecognized control packet type: {:?}", x),
                ))
            }
        }
    }

    fn id_byte(&self) -> u8 {
        match self {
            &ControlTypes::Handshake(_) => 0x0,
            &ControlTypes::KeepAlive => 0x1,
            &ControlTypes::Ack(_, _) => 0x2,
            &ControlTypes::Nak(_) => 0x3,
            &ControlTypes::Shutdown => 0x5,
            &ControlTypes::Ack2(_) => 0x6,
            &ControlTypes::DropRequest(_, _) => 0x7,
        }
    }

    fn add_info(&self) -> i32 {
        match self {
            &ControlTypes::Handshake(_) => 0,
            &ControlTypes::KeepAlive => 0,
            &ControlTypes::Ack(i, _) => i,
            &ControlTypes::Nak(_) => 0,
            &ControlTypes::Shutdown => 0,
            &ControlTypes::Ack2(i) => i,
            &ControlTypes::DropRequest(i, _) => i,
        }
    }

    fn serialize<T: BufMut>(&self, mut into: T) {
        match self {
            &ControlTypes::Handshake(ref c) => {
                into.put_i32::<BigEndian>(c.udt_version);
                into.put_i32::<BigEndian>(c.sock_type.to_i32());
                into.put_i32::<BigEndian>(c.init_seq_num);
                into.put_i32::<BigEndian>(c.max_packet_size);
                into.put_i32::<BigEndian>(c.max_flow_size);
                into.put_i32::<BigEndian>(c.connection_type.to_i32());
                into.put_i32::<BigEndian>(c.socket_id);
                into.put_i32::<BigEndian>(c.syn_cookie);

                match c.peer_addr {
                    IpAddr::V4(four) => {
                        into.put(&four.octets()[..]);

                        into.put(&b"\0\0\0\0"[..]);
                    }
                    IpAddr::V6(six) => into.put(&six.octets()[..]),
                }
            }
            &ControlTypes::KeepAlive => {}
            &ControlTypes::Ack(_, ref a) => unimplemented!(),
            &ControlTypes::Nak(ref n) => unimplemented!(),
            &ControlTypes::Shutdown => {}
            &ControlTypes::Ack2(_) => {}
            &ControlTypes::DropRequest(_, ref d) => unimplemented!(),
        };
    }
}

/// The DropRequest control info
#[derive(Debug)]
pub struct DropRequestControlInfo {
    /// The first message to drop
    pub first: i32,

    /// The last message to drop
    pub last: i32,
}

/// The NAK control info
#[derive(Debug)]
pub struct NakControlInfo {
    /// The loss infomration
    /// If a number in this is a seq number (first bit 0),
    /// then the packet with this sequence is lost
    ///
    /// If a packet that's not a seq number (first bit 1),
    /// then all packets starting from this number (including)
    /// to the number in the next integer (including), which must have a zero first bit.
    pub loss_info: Vec<i32>,
}

/// The ACK control info struct
#[derive(Debug)]
pub struct AckControlInfo {
    /// The packet sequence number that all packets have been recieved until (excluding)
    pub recvd_until: i32,

    /// Round trip time
    pub rtt: Option<i32>,

    /// RTT variance
    pub rtt_variance: Option<i32>,

    /// available buffer
    pub buffer_available: Option<i32>,

    /// receive rate, in packets/sec
    pub packet_recv_rate: Option<i32>,

    /// Estimated Link capacity
    pub est_link_cap: Option<i32>,
}

/// The control info for handshake packets
#[derive(Debug)]
pub struct HandshakeControlInfo {
    /// The UDT version, currently 4
    pub udt_version: i32,

    /// The socket type
    pub sock_type: SocketType,

    /// The initial sequence number, usually randomly initialized
    pub init_seq_num: i32,

    /// Max packet size, including UDP/IP headers. 1500 by default
    pub max_packet_size: i32,

    /// Max flow window size, by default 25600
    pub max_flow_size: i32,

    /// Connection type, either rendezvois (0) or regular (1)
    pub connection_type: ConnectionType,

    /// The socket ID that this request is originating from
    pub socket_id: i32,

    /// SYN cookie
    ///
    /// "generates a cookie value according to the client address and a
    /// secret key and sends it back to the client. The client must then send
    /// back the same cookie to the server."
    pub syn_cookie: i32,

    /// The IP address of the connecting client
    pub peer_addr: IpAddr,
}

/// The socket type for a handshake.
#[derive(Debug)]
pub enum SocketType {
    /// A stream socket, 1 when serialized
    Stream,

    /// A datagram socket, 2 when serialied
    Datagram,
}

impl SocketType {
    pub fn from_i32(num: i32) -> Result<SocketType> {
        match num {
            1 => Ok(SocketType::Stream),
            2 => Ok(SocketType::Datagram),
            i => Err(Error::new(
                ErrorKind::InvalidData,
                format!("Unrecognized socket type: {:?}", i),
            )),
        }
    }

    pub fn to_i32(&self) -> i32 {
        match self {
            &SocketType::Stream => 1,
            &SocketType::Datagram => 2,
        }
    }
}

/// See https://tools.ietf.org/html/draft-gg-udt-03#page-10
#[derive(Debug)]
pub enum ConnectionType {
    /// A regular connection; one listener and one sender, 1
    Regular,

    /// A rendezvous connection, initial connect request, 0
    RendezvousFirst,

    /// A rendezvous connection, response to intial connect request, -1
    RendezvousSecond,

    /// Final rendezvous check, -2
    RendezvousFinal,
}

impl ConnectionType {
    pub fn from_i32(num: i32) -> Result<ConnectionType> {
        match num {
            1 => Ok(ConnectionType::Regular),
            0 => Ok(ConnectionType::RendezvousFirst),
            -1 => Ok(ConnectionType::RendezvousSecond),
            -2 => Ok(ConnectionType::RendezvousFinal),
            i => Err(Error::new(
                ErrorKind::InvalidData,
                format!("Unrecognized connection type: {:?}", i),
            )),
        }
    }

    pub fn to_i32(&self) -> i32 {
        match self {
            &ConnectionType::Regular => 1,
            &ConnectionType::RendezvousFirst => 0,
            &ConnectionType::RendezvousSecond => -1,
            &ConnectionType::RendezvousFinal => -2,
        }
    }
}

impl Packet {
    pub fn parse<T: Buf>(mut buf: T) -> Result<Packet> {
        // Buffer must be at least 16 bytes,
        // the length of a header packet
        if buf.remaining() < 16 {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "Packet not long enough to have a header",
            ));
        }

        // get the first four bytes
        let first4: Vec<_> = (0..4).map(|_| buf.get_u8()).collect();

        // Check if the first bit is one or zero;
        // if it's one it's a cotnrol packet,
        // if zero it's a data packet
        if (first4[0] & 0b1 << 7) == 0 {
            // this means it's a data packet

            // get the sequence number, which is the last 31 bits of the header
            // because the first bit is zero, we can just convert the first 4 bits into a
            // 32 bit integer

            let seq_number = Cursor::new(first4).get_i32::<BigEndian>();

            // get the first byte in the second row
            let second_line = buf.get_i32::<BigEndian>();

            let message_loc = PacketLocation::from_i32(second_line);

            // Third bit of FF is delivery order
            let in_order_delivery = (second_line & 0b1 << 29) != 0;

            // clear the first three bits
            let message_number = second_line & !(0b111 << 29);
            let timestamp = buf.get_i32::<BigEndian>();
            let dest_sockid = buf.get_i32::<BigEndian>();

            Ok(Packet::Data(DataPacket {
                seq_number,
                message_loc,
                in_order_delivery,
                message_number,
                timestamp,
                dest_sockid,
                payload: buf.collect(),
            }))
        } else {
            // this means it's a control packet

            let add_info = buf.get_i32::<BigEndian>();
            let timestamp = buf.get_i32::<BigEndian>();
            let dest_sockid = buf.get_i32::<BigEndian>();

            Ok(Packet::Control(ControlPacket {
                timestamp,
                dest_sockid,
                // just match against the second byte, as everything is in that
                control_type: ControlTypes::deserialize(first4[1], add_info, buf)?,
            }))
        }
    }

    pub fn serialize<T: BufMut>(&self, mut into: T) {
        match self {
            &Packet::Control(ref c) => {
                // first half of first row, the control type and the 1st bit which is a one
                into.put_i16::<BigEndian>((c.control_type.id_byte() as i16) | (0b1 << 15));

                // finish that row, which is reserved, so just fill with zeros
                into.put_i16::<BigEndian>(0);

                // the additonal info line
                into.put_i32::<BigEndian>(c.control_type.add_info());

                // timestamp
                into.put_i32::<BigEndian>(c.timestamp);

                // dest sock id
                into.put_i32::<BigEndian>(c.dest_sockid);

                // the rest of the info
                c.control_type.serialize(into);
            }
            &Packet::Data(ref d) => {
                into.put_i32::<BigEndian>(d.seq_number);
                into.put_i32::<BigEndian>(d.message_number | d.message_loc.to_i32());
                into.put_i32::<BigEndian>(d.timestamp);
                into.put_i32::<BigEndian>(d.dest_sockid);
                into.put(&d.payload);
            }
        }
    }
}

#[test]
fn packet_location_test() {
    assert_eq!(PacketLocation::from_i32(0b10 << 30), PacketLocation::First);
    assert_eq!(PacketLocation::from_i32(!(0b01 << 30)), PacketLocation::First);
    assert_eq!(PacketLocation::from_i32(0b101010101110 << 20), PacketLocation::First);

    assert_eq!(PacketLocation::from_i32(0b00), PacketLocation::Middle);
    assert_eq!(PacketLocation::from_i32(!(0b11 << 30)), PacketLocation::Middle);
    assert_eq!(PacketLocation::from_i32(0b001010101110 << 20), PacketLocation::Middle);

    assert_eq!(PacketLocation::from_i32(0b01 << 30), PacketLocation::Last);
    assert_eq!(PacketLocation::from_i32(!(0b10 << 30)), PacketLocation::Last);
    assert_eq!(PacketLocation::from_i32(0b011100101110 << 20), PacketLocation::Last);

    assert_eq!(PacketLocation::from_i32(0b11 << 30), PacketLocation::Only);
    assert_eq!(PacketLocation::from_i32(!(0b00 << 30)), PacketLocation::Only);
    assert_eq!(PacketLocation::from_i32(0b110100101110 << 20), PacketLocation::Only);
}