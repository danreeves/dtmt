```010editor
//------------------------------------------------
//--- 010 Editor v12.0.1 Binary Template
//
//      File: dt_wwise_bank.bt
//   Authors: Lucas Schwiderski
//   Version: 
//   Purpose: 
//  Category: Darktide
// File Mask: *.wwise_bank
//  ID Bytes: 
//   History: 
//------------------------------------------------

// Definitions for Wwise Bank are taken from the repository.
// Copyright Alexander Lombardi

/*********************************************************************** 
Typedef's Section:
***********************************************************************/ 
typedef struct(uint32 dataLen)
{
    local uint32 i, numFiles = dataLen / 12; // 12 = 3 * uint32 entries.
    for(i = 0; i < numFiles; i++)
    {
        struct d_entry
        {
            uint32 fileId   <format=hex, comment=".wem file id">;
            uint32 dOffset  <comment="offset from start of DATA section">;
            uint32 dSize    <comment="length in bytes of .wem file">;
        } a_wem_file;
    }
} data_DIDX;

typedef struct(data_DIDX &DIDX_dataObj)
{
    local uint32 i;
    local const uint64 pos = FTell();

    for(i = 0; i < DIDX_dataObj.numFiles; i++)
    {
        FSeek(pos + DIDX_dataObj.a_wem_file[i].dOffset);
        struct d_entry_data(data_DIDX &DIDX_dataObj)
        {
            byte rawdata[DIDX_dataObj.a_wem_file[i].dSize] <comment="The .wem file with the length as given in the DIDX section, and starting with 52 49 46 46 -- RIFF.">;
        } aFileData (DIDX_dataObj);
    }
    FSeek(pos + parentof(this).secLen);
} data_DATA;

typedef enum <byte> {
    Settings = 1,
    SoundVoice,
    EventAction,
    Event,
    RandomContainer,
    SwitchContainer,
    ActorMixer,
    AudioBus,
    BlendContainer,
    MusicSegment,
    MusicTrack,
    MusicSwitchContainer,
    MusicPlaylistContainer,
    Attenuation,
    DialogueEvent,
    MotionBus,
    MotionFX,
    Effect,
    Unknown1,
    AuxiliaryBus,
    Unknown2
} data_HIRC_object_type;

typedef struct {
    data_HIRC_object_type object_type;
    uint32 object_length;
    uint32 id <format=hex>;
    byte data[object_length-4];
} data_HIRC_object;

typedef struct {
    uint32 object_count;
    data_HIRC_object objects[object_count] <optimize=false>;
} data_HIRC;

typedef struct
{
    char secName[4];

    if(Strcmp(secName, "ffff") != 0) {
        uint32 secLen <comment="length of section">;
        if(secLen > 0)
        {
           if(Strcmp(secName, "DIDX") == 0)
            {
                data_DIDX data(secLen);
                parentof(this).flag_DIDX_defined = TRUE;
                parentof(this).idxOf_DIDX_sec = parentof(this).i;
            }
            else if(Strcmp(secName, "DATA") == 0 && parentof(this).flag_DIDX_defined)
                data_DATA data(parentof(this).aSection[parentof(this).idxOf_DIDX_sec].data);
            else if(Strcmp(secName, "HIRC") == 0)
                data_HIRC data;
            else
                byte data[secLen];
        };
    };
} section <read=getSecName>;

/*********************************************************************** 
File Definition:
***********************************************************************/ 

uint32 version <format=hex>;
Assert(version == 0x1b);
uint32 length;

// A section of values that don't belong the actual Wwise bank
struct {
    uint32 unknown1;
    Assert(unknown1 == 0x0);
    // Matches the file itself
    uint64 this_file <format=hex>;
    uint32 unknown2;
    Assert(unknown2 == 0x20);
    uint32 unknown3;
    Assert(unknown3 == 0x0);
} header;

struct bnk
{
    local byte flag_DIDX_defined = FALSE;   // Used later to ensure DATA section is defined and declared after DIDX.
    local int32 i;                          // i is used later to find the index of the DIDX section.
    local int32 idxOf_DIDX_sec = -1;        // Will get set to i's value when DIDX gets parsed.

    for(i = 0; !FEof(); i++)
        section aSection;
} wwise_bank <open=true>;


/*********************************************************************** 
Function definitions:
***********************************************************************/
string getSecName(section &aSec)
{
    return aSec.secName;
}
```