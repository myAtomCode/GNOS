pub const MAX_ENDPOINTS: usize = 8;
pub const MAX_CAPABILITIES: usize = 64;
pub const MAX_REPLY_TOKENS: usize = 32;
pub const QUEUE_DEPTH: usize = 8;
pub const MAX_PAYLOAD_BYTES: usize = 192;
pub const MAX_PAGE_LOANS: usize = 16;
pub const MAX_LOAN_PAGES: usize = 8;
pub const IPC_ABI_VERSION: u64 = 2;
pub const SYS_IPC_ABI_INFO: u64 = 0x1100;
pub const SYS_ENDPOINT_CREATE: u64 = 0x1101;
pub const SYS_SERVICE_LOOKUP: u64 = 0x1102;
pub const SYS_IPC_CALL_SEND: u64 = 0x1103;
pub const SYS_IPC_RECEIVE: u64 = 0x1104;
pub const SYS_IPC_REPLY: u64 = 0x1105;
pub const SYS_IPC_LOAN_SEND: u64 = 0x1106;
pub const SYS_IPC_LOAN_MAP: u64 = 0x1107;
pub const SERVICE_VFS: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum RequestKind {
    ConsoleWrite = 1,
    ConsolePrompt = 2,
    VfsReadFile = 3,
    VfsWriteFile = 4,
    VfsListDir = 5,
    ProcExec = 6,
    KernelListServices = 7,
    KernelListTasks = 8,
    NetStatus = 9,
    NetPing = 10,
    ProcFork = 11,
    ProcClone = 12,
    ProcExit = 13,
    ProcWait4 = 14,
    ProcSignal = 15,
    ProcFutex = 16,
    ProcSched = 17,
    ProcAffinity = 18,
    ProcWaitQueue = 19,
    ProcTimer = 20,
}

impl RequestKind {
    pub const fn from_opcode(opcode: u16) -> Option<Self> {
        match opcode {
            1 => Some(Self::ConsoleWrite),
            2 => Some(Self::ConsolePrompt),
            3 => Some(Self::VfsReadFile),
            4 => Some(Self::VfsWriteFile),
            5 => Some(Self::VfsListDir),
            6 => Some(Self::ProcExec),
            7 => Some(Self::KernelListServices),
            8 => Some(Self::KernelListTasks),
            9 => Some(Self::NetStatus),
            10 => Some(Self::NetPing),
            11 => Some(Self::ProcFork),
            12 => Some(Self::ProcClone),
            13 => Some(Self::ProcExit),
            14 => Some(Self::ProcWait4),
            15 => Some(Self::ProcSignal),
            16 => Some(Self::ProcFutex),
            17 => Some(Self::ProcSched),
            18 => Some(Self::ProcAffinity),
            19 => Some(Self::ProcWaitQueue),
            20 => Some(Self::ProcTimer),
            _ => None,
        }
    }

    pub const fn opcode(self) -> u16 {
        self as u16
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            RequestKind::ConsoleWrite => "write",
            RequestKind::ConsolePrompt => "read",
            RequestKind::VfsReadFile => "pread64",
            RequestKind::VfsWriteFile => "pwrite64",
            RequestKind::VfsListDir => "getdents64",
            RequestKind::ProcExec => "execve",
            RequestKind::KernelListServices => "service_list",
            RequestKind::KernelListTasks => "task_list",
            RequestKind::NetStatus => "rtnetlink_getlink",
            RequestKind::NetPing => "icmp_echo",
            RequestKind::ProcFork => "fork",
            RequestKind::ProcClone => "clone",
            RequestKind::ProcExit => "exit",
            RequestKind::ProcWait4 => "wait4",
            RequestKind::ProcSignal => "rt_sigqueueinfo",
            RequestKind::ProcFutex => "futex",
            RequestKind::ProcSched => "sched_setscheduler",
            RequestKind::ProcAffinity => "sched_setaffinity",
            RequestKind::ProcWaitQueue => "wait_queue",
            RequestKind::ProcTimer => "timer_settime",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct EndpointId(u64);

impl EndpointId {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CapabilityId(u64);

impl CapabilityId {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ReplyTokenId(u64);

impl ReplyTokenId {
    pub const NONE: Self = Self(0);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PageLoanId(u64);

impl PageLoanId {
    pub const NONE: Self = Self(0);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct IpcRights(u8);

impl IpcRights {
    pub const SEND: Self = Self(1 << 0);
    pub const RECEIVE: Self = Self(1 << 1);
    pub const MANAGE: Self = Self(1 << 2);
    const OWNER: Self = Self(Self::SEND.0 | Self::RECEIVE.0 | Self::MANAGE.0);

    const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcError {
    InvalidOwner,
    EndpointTableFull,
    CapabilityTableFull,
    ReplyTokenTableFull,
    InvalidEndpoint,
    InvalidCapability,
    InvalidReplyToken,
    PermissionDenied,
    InvalidRights,
    PayloadTooLarge,
    QueueFull,
    QueueEmpty,
    MessageIdExhausted,
    ProtocolViolation,
    PageLoanTableFull,
    InvalidPageLoan,
    InvalidPageRange,
}

impl IpcError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidOwner => "ipc: owner PID must be nonzero",
            Self::EndpointTableFull => "ipc: endpoint table is full",
            Self::CapabilityTableFull => "ipc: capability table is full",
            Self::ReplyTokenTableFull => "ipc: reply token table is full",
            Self::InvalidEndpoint => "ipc: invalid or stale endpoint",
            Self::InvalidCapability => "ipc: invalid or stale capability",
            Self::InvalidReplyToken => "ipc: invalid, stale, or consumed reply token",
            Self::PermissionDenied => "ipc: capability permission denied",
            Self::InvalidRights => "ipc: invalid capability rights",
            Self::PayloadTooLarge => "ipc: message payload is too large",
            Self::QueueFull => "ipc: endpoint queue is full",
            Self::QueueEmpty => "ipc: endpoint queue is empty",
            Self::MessageIdExhausted => "ipc: message id space exhausted",
            Self::ProtocolViolation => "ipc: message protocol violation",
            Self::PageLoanTableFull => "ipc: page loan table is full",
            Self::InvalidPageLoan => "ipc: invalid, stale, or unauthorized page loan",
            Self::InvalidPageRange => "ipc: invalid page loan range",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message {
    opcode: u16,
    length: u16,
    loan: PageLoanId,
    payload: [u8; MAX_PAYLOAD_BYTES],
}

impl Message {
    pub fn new(kind: RequestKind, payload: &[u8]) -> Result<Self, IpcError> {
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(IpcError::PayloadTooLarge);
        }
        let mut message = Self {
            opcode: kind.opcode(),
            length: payload.len() as u16,
            loan: PageLoanId::NONE,
            payload: [0; MAX_PAYLOAD_BYTES],
        };
        message.payload[..payload.len()].copy_from_slice(payload);
        Ok(message)
    }

    pub const fn opcode(&self) -> u16 {
        self.opcode
    }

    pub fn payload(&self) -> &[u8] {
        if self.loan == PageLoanId::NONE {
            &self.payload[..usize::from(self.length)]
        } else {
            &[]
        }
    }

    pub fn page_loan(kind: RequestKind, loan: PageLoanId, length: usize) -> Result<Self, IpcError> {
        if loan == PageLoanId::NONE || length == 0 || length > MAX_LOAN_PAGES * 4096 {
            return Err(IpcError::InvalidPageRange);
        }
        Ok(Self {
            opcode: kind.opcode(),
            length: length as u16,
            loan,
            payload: [0; MAX_PAYLOAD_BYTES],
        })
    }

    pub const fn loan(&self) -> PageLoanId {
        self.loan
    }

    pub const fn transferred_len(&self) -> usize {
        self.length as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageLoan {
    pub owner: u32,
    pub recipient: u32,
    pub length: u16,
    pub pages: u8,
    frames: [u64; MAX_LOAN_PAGES],
}

impl PageLoan {
    pub const fn frame(&self, index: usize) -> Option<u64> {
        if index < self.pages as usize {
            Some(self.frames[index])
        } else {
            None
        }
    }
}

#[derive(Clone, Copy)]
struct PageLoanSlot {
    generation: u32,
    loan: Option<PageLoan>,
}

impl PageLoanSlot {
    const EMPTY: Self = Self {
        generation: 0,
        loan: None,
    };
}

pub struct PageLoanTable {
    slots: [PageLoanSlot; MAX_PAGE_LOANS],
}

impl PageLoanTable {
    pub const fn new() -> Self {
        Self {
            slots: [PageLoanSlot::EMPTY; MAX_PAGE_LOANS],
        }
    }

    pub fn create(
        &mut self,
        owner: u32,
        recipient: u32,
        frames: [u64; MAX_LOAN_PAGES],
        pages: usize,
        length: usize,
    ) -> Result<PageLoanId, IpcError> {
        if owner == 0
            || recipient == 0
            || pages == 0
            || pages > MAX_LOAN_PAGES
            || length == 0
            || length > pages * 4096
            || length <= (pages - 1) * 4096
            || frames[..pages].iter().any(|frame| frame & 4095 != 0)
        {
            return Err(IpcError::InvalidPageRange);
        }
        let index = self
            .slots
            .iter()
            .position(|slot| slot.loan.is_none() && slot.generation != u32::MAX)
            .ok_or(IpcError::PageLoanTableFull)?;
        let generation = active_generation(self.slots[index].generation);
        self.slots[index].generation = generation;
        self.slots[index].loan = Some(PageLoan {
            owner,
            recipient,
            length: length as u16,
            pages: pages as u8,
            frames,
        });
        Ok(PageLoanId(encode_handle(index, generation)))
    }

    pub fn get(&self, recipient: u32, id: PageLoanId) -> Result<PageLoan, IpcError> {
        let (index, generation) =
            decode_handle(id.raw(), self.slots.len()).ok_or(IpcError::InvalidPageLoan)?;
        let slot = &self.slots[index];
        if slot.generation != generation {
            return Err(IpcError::InvalidPageLoan);
        }
        let loan = slot.loan.ok_or(IpcError::InvalidPageLoan)?;
        if loan.recipient != recipient {
            return Err(IpcError::InvalidPageLoan);
        }
        Ok(loan)
    }

    pub fn consume(&mut self, recipient: u32, id: PageLoanId) -> Result<PageLoan, IpcError> {
        let loan = self.get(recipient, id)?;
        let (index, _) =
            decode_handle(id.raw(), self.slots.len()).ok_or(IpcError::InvalidPageLoan)?;
        let slot = &mut self.slots[index];
        slot.loan = None;
        slot.generation = next_generation(slot.generation);
        Ok(loan)
    }

    pub fn drain_subject(&mut self, subject: u32, mut release: impl FnMut(PageLoan)) {
        for slot in &mut self.slots {
            if slot.loan.is_some_and(|loan| loan.recipient == subject) {
                if let Some(loan) = slot.loan.take() {
                    release(loan);
                }
                slot.generation = next_generation(slot.generation);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Envelope {
    pub id: u64,
    pub sender: u32,
    pub reply_token: ReplyTokenId,
    pub message: Message,
}

#[derive(Clone, Copy)]
struct Endpoint {
    owner: u32,
    next_message_id: u64,
    head: usize,
    length: usize,
    queue: [Option<Envelope>; QUEUE_DEPTH],
}

impl Endpoint {
    const fn new(owner: u32) -> Self {
        Self {
            owner,
            next_message_id: 1,
            head: 0,
            length: 0,
            queue: [None; QUEUE_DEPTH],
        }
    }

    fn push(
        &mut self,
        sender: u32,
        reply_token: ReplyTokenId,
        message: Message,
    ) -> Result<u64, IpcError> {
        if self.length == self.queue.len() {
            return Err(IpcError::QueueFull);
        }
        let id = self.next_message_id;
        self.next_message_id = self
            .next_message_id
            .checked_add(1)
            .ok_or(IpcError::MessageIdExhausted)?;
        let tail = (self.head + self.length) % self.queue.len();
        self.queue[tail] = Some(Envelope {
            id,
            sender,
            reply_token,
            message,
        });
        self.length += 1;
        Ok(id)
    }

    fn pop(&mut self) -> Result<Envelope, IpcError> {
        if self.length == 0 {
            return Err(IpcError::QueueEmpty);
        }
        let envelope = self.queue[self.head].take().ok_or(IpcError::QueueEmpty)?;
        self.head = (self.head + 1) % self.queue.len();
        self.length -= 1;
        Ok(envelope)
    }
}

#[derive(Clone, Copy)]
struct EndpointSlot {
    generation: u32,
    endpoint: Option<Endpoint>,
}

impl EndpointSlot {
    const EMPTY: Self = Self {
        generation: 0,
        endpoint: None,
    };
}

#[derive(Clone, Copy)]
struct Capability {
    subject: u32,
    endpoint: EndpointId,
    rights: IpcRights,
}

#[derive(Clone, Copy)]
struct CapabilitySlot {
    generation: u32,
    capability: Option<Capability>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReplyToken {
    requester: u32,
    replier: u32,
    reply_endpoint: EndpointId,
}

#[derive(Clone, Copy)]
struct ReplyTokenSlot {
    generation: u32,
    token: Option<ReplyToken>,
}

impl ReplyTokenSlot {
    const EMPTY: Self = Self {
        generation: 0,
        token: None,
    };
}

impl CapabilitySlot {
    const EMPTY: Self = Self {
        generation: 0,
        capability: None,
    };
}

pub struct IpcTable {
    endpoints: [EndpointSlot; MAX_ENDPOINTS],
    capabilities: [CapabilitySlot; MAX_CAPABILITIES],
    reply_tokens: [ReplyTokenSlot; MAX_REPLY_TOKENS],
}

static IPC_SELF_TEST: crate::sync::SpinMutex<IpcTable> =
    crate::sync::SpinMutex::new(IpcTable::new());

impl IpcTable {
    pub const fn new() -> Self {
        Self {
            endpoints: [EndpointSlot::EMPTY; MAX_ENDPOINTS],
            capabilities: [CapabilitySlot::EMPTY; MAX_CAPABILITIES],
            reply_tokens: [ReplyTokenSlot::EMPTY; MAX_REPLY_TOKENS],
        }
    }

    fn reset(&mut self) {
        for slot in &mut self.endpoints {
            *slot = EndpointSlot::EMPTY;
        }
        for slot in &mut self.capabilities {
            *slot = CapabilitySlot::EMPTY;
        }
        for slot in &mut self.reply_tokens {
            *slot = ReplyTokenSlot::EMPTY;
        }
    }

    pub fn create_endpoint(&mut self, owner: u32) -> Result<(EndpointId, CapabilityId), IpcError> {
        if owner == 0 {
            return Err(IpcError::InvalidOwner);
        }
        let endpoint_index = self
            .endpoints
            .iter()
            .position(|slot| slot.endpoint.is_none() && slot.generation != u32::MAX)
            .ok_or(IpcError::EndpointTableFull)?;
        let capability_index = self
            .capabilities
            .iter()
            .position(|slot| slot.capability.is_none() && slot.generation != u32::MAX)
            .ok_or(IpcError::CapabilityTableFull)?;
        let endpoint_generation = active_generation(self.endpoints[endpoint_index].generation);
        self.endpoints[endpoint_index].generation = endpoint_generation;
        let capability_generation =
            active_generation(self.capabilities[capability_index].generation);
        self.capabilities[capability_index].generation = capability_generation;
        let endpoint = encode_handle(endpoint_index, endpoint_generation);
        self.endpoints[endpoint_index].endpoint = Some(Endpoint::new(owner));
        let capability = Capability {
            subject: owner,
            endpoint: EndpointId(endpoint),
            rights: IpcRights::OWNER,
        };
        self.capabilities[capability_index].capability = Some(capability);
        Ok((
            EndpointId(endpoint),
            CapabilityId(encode_handle(capability_index, capability_generation)),
        ))
    }

    pub fn grant(
        &mut self,
        caller: u32,
        authority: CapabilityId,
        subject: u32,
        rights: IpcRights,
    ) -> Result<CapabilityId, IpcError> {
        if subject == 0 || rights != IpcRights::SEND {
            return Err(IpcError::InvalidRights);
        }
        let endpoint = self.resolve_capability(caller, authority, IpcRights::MANAGE)?;
        if let Some(existing) = self.capability_for(subject, endpoint, rights) {
            return Ok(existing);
        }
        let index = self
            .capabilities
            .iter()
            .position(|slot| slot.capability.is_none() && slot.generation != u32::MAX)
            .ok_or(IpcError::CapabilityTableFull)?;
        let generation = active_generation(self.capabilities[index].generation);
        self.capabilities[index].generation = generation;
        self.capabilities[index].capability = Some(Capability {
            subject,
            endpoint,
            rights,
        });
        Ok(CapabilityId(encode_handle(index, generation)))
    }

    pub fn capability_for(
        &self,
        subject: u32,
        endpoint: EndpointId,
        rights: IpcRights,
    ) -> Option<CapabilityId> {
        self.endpoint(endpoint)?;
        self.capabilities
            .iter()
            .enumerate()
            .find_map(|(index, slot)| {
                slot.capability
                    .filter(|capability| {
                        capability.subject == subject
                            && capability.endpoint == endpoint
                            && capability.rights.contains(rights)
                    })
                    .map(|_| CapabilityId(encode_handle(index, slot.generation)))
            })
    }

    pub fn send(
        &mut self,
        caller: u32,
        capability: CapabilityId,
        message: Message,
    ) -> Result<u64, IpcError> {
        let endpoint = self.resolve_capability(caller, capability, IpcRights::SEND)?;
        self.endpoint_mut(endpoint)?
            .push(caller, ReplyTokenId::NONE, message)
    }

    pub fn call(
        &mut self,
        caller: u32,
        capability: CapabilityId,
        reply_capability: CapabilityId,
        message: Message,
    ) -> Result<u64, IpcError> {
        let destination = self.resolve_capability(caller, capability, IpcRights::SEND)?;
        let reply_endpoint =
            self.resolve_capability(caller, reply_capability, IpcRights::RECEIVE)?;
        let replier = self
            .endpoint(destination)
            .map(|endpoint| endpoint.owner)
            .ok_or(IpcError::InvalidEndpoint)?;
        let token = self.allocate_reply_token(ReplyToken {
            requester: caller,
            replier,
            reply_endpoint,
        })?;
        match self.endpoint_mut(destination)?.push(caller, token, message) {
            Ok(message_id) => Ok(message_id),
            Err(error) => {
                self.revoke_reply_token(token);
                Err(error)
            }
        }
    }

    pub fn reply(
        &mut self,
        caller: u32,
        token: ReplyTokenId,
        message: Message,
    ) -> Result<u64, IpcError> {
        let (index, generation, entry) = self.resolve_reply_token(caller, token)?;
        let message_id =
            self.endpoint_mut(entry.reply_endpoint)?
                .push(caller, ReplyTokenId::NONE, message)?;
        let slot = &mut self.reply_tokens[index];
        if slot.generation != generation || slot.token != Some(entry) {
            return Err(IpcError::InvalidReplyToken);
        }
        slot.token = None;
        slot.generation = next_generation(slot.generation);
        Ok(message_id)
    }

    pub fn authorize_send(&self, caller: u32, capability: CapabilityId) -> Result<(), IpcError> {
        self.resolve_capability(caller, capability, IpcRights::SEND)
            .map(|_| ())
    }

    pub fn send_target(&self, caller: u32, capability: CapabilityId) -> Result<u32, IpcError> {
        let endpoint = self.resolve_capability(caller, capability, IpcRights::SEND)?;
        self.endpoint(endpoint)
            .map(|endpoint| endpoint.owner)
            .ok_or(IpcError::InvalidEndpoint)
    }

    pub fn authorize_call(
        &self,
        caller: u32,
        capability: CapabilityId,
        reply_capability: CapabilityId,
    ) -> Result<(), IpcError> {
        self.resolve_capability(caller, capability, IpcRights::SEND)?;
        self.resolve_capability(caller, reply_capability, IpcRights::RECEIVE)?;
        Ok(())
    }

    pub fn authorize_reply(&self, caller: u32, token: ReplyTokenId) -> Result<(), IpcError> {
        self.resolve_reply_token(caller, token).map(|_| ())
    }

    pub fn receive(&mut self, caller: u32, capability: CapabilityId) -> Result<Envelope, IpcError> {
        let endpoint = self.resolve_capability(caller, capability, IpcRights::RECEIVE)?;
        self.endpoint_mut(endpoint)?.pop()
    }

    pub fn close(&mut self, caller: u32, capability: CapabilityId) -> Result<(), IpcError> {
        let endpoint = self.resolve_capability(caller, capability, IpcRights::MANAGE)?;
        let (endpoint_index, generation) =
            decode_handle(endpoint.raw(), self.endpoints.len()).ok_or(IpcError::InvalidEndpoint)?;
        let slot = &mut self.endpoints[endpoint_index];
        if slot.generation != generation || slot.endpoint.map(|entry| entry.owner) != Some(caller) {
            return Err(IpcError::PermissionDenied);
        }
        slot.endpoint = None;
        slot.generation = next_generation(slot.generation);
        for capability_slot in &mut self.capabilities {
            if capability_slot
                .capability
                .is_some_and(|entry| entry.endpoint == endpoint)
            {
                capability_slot.capability = None;
                capability_slot.generation = next_generation(capability_slot.generation);
            }
        }
        for token_slot in &mut self.reply_tokens {
            if token_slot
                .token
                .is_some_and(|entry| entry.reply_endpoint == endpoint || entry.replier == caller)
            {
                token_slot.token = None;
                token_slot.generation = next_generation(token_slot.generation);
            }
        }
        Ok(())
    }

    pub fn retain_subjects(&mut self, mut retain: impl FnMut(u32) -> bool) {
        for slot in &mut self.capabilities {
            if slot
                .capability
                .is_some_and(|capability| !retain(capability.subject))
            {
                slot.capability = None;
                slot.generation = next_generation(slot.generation);
            }
        }
        for slot in &mut self.reply_tokens {
            if slot
                .token
                .is_some_and(|token| !retain(token.requester) || !retain(token.replier))
            {
                slot.token = None;
                slot.generation = next_generation(slot.generation);
            }
        }
    }

    pub fn remove_subject(&mut self, subject: u32) {
        for index in 0..self.endpoints.len() {
            if self.endpoints[index]
                .endpoint
                .is_none_or(|endpoint| endpoint.owner != subject)
            {
                continue;
            }
            let endpoint = EndpointId(encode_handle(index, self.endpoints[index].generation));
            self.endpoints[index].endpoint = None;
            self.endpoints[index].generation = next_generation(self.endpoints[index].generation);
            for slot in &mut self.capabilities {
                if slot
                    .capability
                    .is_some_and(|capability| capability.endpoint == endpoint)
                {
                    slot.capability = None;
                    slot.generation = next_generation(slot.generation);
                }
            }
        }
        for slot in &mut self.capabilities {
            if slot
                .capability
                .is_some_and(|capability| capability.subject == subject)
            {
                slot.capability = None;
                slot.generation = next_generation(slot.generation);
            }
        }
        for slot in &mut self.reply_tokens {
            if slot
                .token
                .is_some_and(|token| token.requester == subject || token.replier == subject)
            {
                slot.token = None;
                slot.generation = next_generation(slot.generation);
            }
        }
    }

    fn resolve_capability(
        &self,
        caller: u32,
        capability: CapabilityId,
        required: IpcRights,
    ) -> Result<EndpointId, IpcError> {
        let (index, generation) = decode_handle(capability.raw(), self.capabilities.len())
            .ok_or(IpcError::InvalidCapability)?;
        let slot = &self.capabilities[index];
        if slot.generation != generation {
            return Err(IpcError::InvalidCapability);
        }
        let entry = slot.capability.ok_or(IpcError::InvalidCapability)?;
        if entry.subject != caller || !entry.rights.contains(required) {
            return Err(IpcError::PermissionDenied);
        }
        self.endpoint(entry.endpoint)
            .ok_or(IpcError::InvalidEndpoint)?;
        Ok(entry.endpoint)
    }

    fn endpoint(&self, endpoint: EndpointId) -> Option<&Endpoint> {
        let (index, generation) = decode_handle(endpoint.raw(), self.endpoints.len())?;
        let slot = &self.endpoints[index];
        (slot.generation == generation)
            .then_some(slot.endpoint.as_ref())
            .flatten()
    }

    fn endpoint_mut(&mut self, endpoint: EndpointId) -> Result<&mut Endpoint, IpcError> {
        let (index, generation) =
            decode_handle(endpoint.raw(), self.endpoints.len()).ok_or(IpcError::InvalidEndpoint)?;
        let slot = &mut self.endpoints[index];
        if slot.generation != generation {
            return Err(IpcError::InvalidEndpoint);
        }
        slot.endpoint.as_mut().ok_or(IpcError::InvalidEndpoint)
    }

    fn allocate_reply_token(&mut self, token: ReplyToken) -> Result<ReplyTokenId, IpcError> {
        let index = self
            .reply_tokens
            .iter()
            .position(|slot| slot.token.is_none() && slot.generation != u32::MAX)
            .ok_or(IpcError::ReplyTokenTableFull)?;
        let generation = active_generation(self.reply_tokens[index].generation);
        self.reply_tokens[index].generation = generation;
        self.reply_tokens[index].token = Some(token);
        Ok(ReplyTokenId(encode_handle(index, generation)))
    }

    fn resolve_reply_token(
        &self,
        caller: u32,
        token: ReplyTokenId,
    ) -> Result<(usize, u32, ReplyToken), IpcError> {
        let (index, generation) = decode_handle(token.raw(), self.reply_tokens.len())
            .ok_or(IpcError::InvalidReplyToken)?;
        let slot = &self.reply_tokens[index];
        if slot.generation != generation {
            return Err(IpcError::InvalidReplyToken);
        }
        let entry = slot.token.ok_or(IpcError::InvalidReplyToken)?;
        if entry.replier != caller {
            return Err(IpcError::PermissionDenied);
        }
        self.endpoint(entry.reply_endpoint)
            .ok_or(IpcError::InvalidEndpoint)?;
        Ok((index, generation, entry))
    }

    fn revoke_reply_token(&mut self, token: ReplyTokenId) {
        let Some((index, generation)) = decode_handle(token.raw(), self.reply_tokens.len()) else {
            return;
        };
        let slot = &mut self.reply_tokens[index];
        if slot.generation == generation && slot.token.is_some() {
            slot.token = None;
            slot.generation = next_generation(slot.generation);
        }
    }
}

#[inline(never)]
pub fn self_test() -> bool {
    let mut table = IPC_SELF_TEST.lock();
    table.reset();
    let Ok((endpoint, owner)) = table.create_endpoint(10) else {
        return false;
    };
    let Ok(sender) = table.grant(10, owner, 20, IpcRights::SEND) else {
        return false;
    };
    if EndpointId::from_raw(endpoint.raw()) != endpoint
        || table.grant(10, owner, 20, IpcRights::MANAGE) != Err(IpcError::InvalidRights)
        || Message::new(RequestKind::ConsoleWrite, &[0; MAX_PAYLOAD_BYTES + 1])
            != Err(IpcError::PayloadTooLarge)
    {
        return false;
    }
    let Ok(message) = Message::new(RequestKind::ConsoleWrite, b"hello") else {
        return false;
    };
    if table.send(21, sender, message) != Err(IpcError::PermissionDenied)
        || table.receive(20, sender) != Err(IpcError::PermissionDenied)
        || table
            .send(
                20,
                CapabilityId::from_raw(sender.raw() ^ (1 << 32)),
                message,
            )
            .is_ok()
    {
        return false;
    }
    let Ok(id) = table.send(20, sender, message) else {
        return false;
    };
    let Ok(received) = table.receive(10, owner) else {
        return false;
    };
    if received.id != id
        || received.sender != 20
        || received.reply_token != ReplyTokenId::NONE
        || received.message.opcode() != RequestKind::ConsoleWrite.opcode()
        || received.message.payload() != b"hello"
    {
        return false;
    }
    let Ok((_, reply_owner)) = table.create_endpoint(20) else {
        return false;
    };
    let Ok(call_id) = table.call(20, sender, reply_owner, message) else {
        return false;
    };
    let Ok(request) = table.receive(10, owner) else {
        return false;
    };
    if request.id != call_id
        || request.reply_token == ReplyTokenId::NONE
        || table.authorize_reply(20, request.reply_token) != Err(IpcError::PermissionDenied)
    {
        return false;
    }
    let Ok(response) = Message::new(RequestKind::ConsoleWrite, b"world") else {
        return false;
    };
    if table.reply(10, request.reply_token, response).is_err()
        || table.reply(10, request.reply_token, response) != Err(IpcError::InvalidReplyToken)
    {
        return false;
    }
    let Ok(reply) = table.receive(20, reply_owner) else {
        return false;
    };
    if reply.sender != 10
        || reply.reply_token != ReplyTokenId::NONE
        || reply.message.payload() != b"world"
    {
        return false;
    }
    for _ in 0..QUEUE_DEPTH {
        if table.send(20, sender, message).is_err() {
            return false;
        }
    }
    if table.send(20, sender, message) != Err(IpcError::QueueFull) {
        return false;
    }
    if table.close(10, owner).is_err() || table.send(20, sender, message).is_ok() {
        return false;
    }
    let Ok((replacement, _)) = table.create_endpoint(10) else {
        return false;
    };
    let mut loans = PageLoanTable::new();
    let mut frames = [0u64; MAX_LOAN_PAGES];
    frames[0] = 0x1000;
    frames[1] = 0x2000;
    let Ok(loan_id) = loans.create(20, 10, frames, 2, 4097) else {
        return false;
    };
    let Ok(loan_message) = Message::page_loan(RequestKind::VfsReadFile, loan_id, 4097) else {
        return false;
    };
    replacement != endpoint
        && loan_message.payload().is_empty()
        && loan_message.transferred_len() == 4097
        && loan_message.loan() == loan_id
        && loans.get(20, loan_id) == Err(IpcError::InvalidPageLoan)
        && loans
            .get(10, loan_id)
            .is_ok_and(|loan| loan.frame(1) == Some(0x2000))
        && loans.consume(10, loan_id).is_ok()
        && loans.consume(10, loan_id) == Err(IpcError::InvalidPageLoan)
}

const fn encode_handle(index: usize, generation: u32) -> u64 {
    ((generation as u64) << 32) | (index as u64 + 1)
}

fn decode_handle(raw: u64, length: usize) -> Option<(usize, u32)> {
    let slot = u32::try_from(raw & u64::from(u32::MAX)).ok()?;
    let generation = u32::try_from(raw >> 32).ok()?;
    if slot == 0 || generation == 0 {
        return None;
    }
    let index = usize::try_from(slot - 1).ok()?;
    (index < length).then_some((index, generation))
}

const fn next_generation(current: u32) -> u32 {
    current.saturating_add(1)
}

const fn active_generation(current: u32) -> u32 {
    if current == 0 {
        1
    } else {
        current
    }
}
