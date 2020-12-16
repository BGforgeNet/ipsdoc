#!/usr/bin/env python3

import argparse
import wave
import struct
import platform
import shutil
import subprocess
import sys, os
import ipsdoc_bin
import base64


parser = argparse.ArgumentParser(description='Convert sound file to ACM', formatter_class=argparse.ArgumentDefaultsHelpFormatter)
parser.add_argument('ifile', help='input WAV, ACM or 8/16 bit raw PCM file')
parser.add_argument('ofile', help='output ACM file', nargs="?")
args = parser.parse_args()

ifile = args.ifile
ofile = args.ofile
if ofile is None:
  ofile = ifile.lower().rstrip(".wav") + ".acm"
  print("output file not specified, defaulting to {}".format(ofile))

acm_bitrate_off = 10
acm_bitrate_len = 2
snd2acm_name = "snd2acm.exe"

def check_reqs():
  system = platform.system()
  if system != 'Windows':
    wine = shutil.which("wine")
    if wine is None:
      print("wine not present in PATH, aborting")
      sys.exit(1)

def get_exe_string(src_path, dst_path):
  cur_dir = os.path.dirname(sys.argv[0])
  system = platform.system()
  wine = False
  exe = False
  if dst_path.lower().endswith(".acm"):
    exe = os.path.join(cur_dir, snd2acm_name)
    exe = exe + " " + src_path + " " + dst_path + " -q0"
  if not exe:
    print("Unknown output file format, aborting")
    sys.exit(1)
  if system != 'Windows':
    wine = shutil.which("wine")
    exe = wine + " " + exe
  return exe

def get_wav_rate(fname):
  with wave.open(fname, mode=None) as wav:
    wav_rate = wav.getframerate()
  print("source bitrate is {}".format(wav_rate))
  return wav_rate

def get_acm_rate(fname):
  with open(fname, 'rb') as ofh:
    odata = ofh.read()
  bitrate = odata[acm_bitrate_off:acm_bitrate_off+acm_bitrate_len]
  acm_rate = struct.unpack('<H', bitrate)[0]
  print("output bitrate is {}".format(acm_rate))
  return acm_rate

def fix_acm_rate(src_rate, dst_rate, fname):
  if src_rate == dst_rate:
    sys.exit()
  print("rate mismatch, correcting")
  bitrate = struct.pack('<H', src_rate)
  with open(fname, 'r+b') as fh:
    fh.seek(acm_bitrate_off)
    fh.write(bitrate)

def convert(src_path, dst_path):
  exe = get_exe_string(src_path, dst_path)
  subprocess.run(exe, shell=True)

def create_bin():
  cur_dir = os.path.dirname(sys.argv[0])
  snd2acm_path = os.path.join(cur_dir, snd2acm_name)
  if not os.path.isfile(snd2acm_path):
    with open(snd2acm_path, "wb") as fh:
      fh.write(base64.b64decode(ipsdoc_bin.snd2acm_base64))

check_reqs()
create_bin()
convert(ifile, ofile)
wav_rate = get_wav_rate(ifile)
acm_rate = get_acm_rate(ofile)
fix_acm_rate(wav_rate, acm_rate, ofile)
